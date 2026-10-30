//! The glue of the player report with [`AppState`] (spec M7-3, slice b):
//! the dialog, one window, one job, the stale test, the filter by click, the
//! Clipbase, save and view.
//!
//! Everything here is bookkeeping, which `update` may do; the selection of
//! the games, the report and the files are [`PlayerReportEffect`]s run by
//! `perform`. The filter of the slot is never touched except by a click on
//! an element of the text (M7-3/D3653).

use std::path::{Path, PathBuf};

use mychess_db::report::select_games;

use super::AppState;
use crate::bases::{CopyList, ListJobKind, OpenControl, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::AppNotice;
use crate::opening_report::SaveFailure;
use crate::opening_report::{ReportFormat, ReportPart, ReportSelector};
use crate::player_info_ui::PlayerInfoMessage;
use crate::player_report::{
    HISTORY_SHOWN, Job, Params, PlayerColor, PlayerComputeJob, PlayerDialogView, PlayerFailure,
    PlayerOutcome, PlayerPhase, PlayerRenderJob, PlayerReportEffect, PlayerReportMessage,
    PlayerReportNotice, PlayerReportPrefs, PlayerReportView, PlayerRequest, PlayerSaveJob,
    PlayerStart, Shown,
};
use crate::prefs::{PrefChange, PrefsMessage};

use super::opening_report_flow::with_report_extension;

/// A name a report can be made for: it has a letter or a digit (the engine
/// refuses the others, M7-3/D3677).
fn usable_name(name: &str) -> bool {
    name.trim().chars().any(char::is_alphanumeric)
}

/// The name of a side of the game, when it names somebody.
fn game_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name != "?").then(|| name.to_owned())
}

impl AppState {
    fn prep_notice(&mut self, notice: PlayerReportNotice) {
        self.notice = Some(AppNotice::PlayerReport(notice));
    }

    /// The state of the player report.
    pub fn player_report(&self) -> &crate::player_report::PlayerReportUi {
        &self.player_report
    }

    /// Sets the date the reports are made on (`None`: the clock). For tests
    /// and the host.
    pub fn set_player_report_today(&mut self, today: Option<mychess_core::PartialDate>) {
        self.player_report.today = today;
    }

    /// Stops the job (the program is ending).
    pub(crate) fn player_report_cancel(&mut self) {
        self.player_report.cancel();
    }

    /// What the front-end needs to keep its model of the report in step:
    /// the shared report, the options it was made with and the database
    /// name.
    pub fn player_report_shown(
        &self,
    ) -> Option<(
        crate::player_report::SharedPlayerReport,
        &mychess_db::report::PlayerReportOptions,
        &str,
    )> {
        let shown = self.player_report.shown.as_ref()?;
        Some((
            shown.report.clone(),
            &shown.options,
            shown.database_name.as_str(),
        ))
    }

    /// The window as it is drawn.
    pub fn player_report_view(&self) -> PlayerReportView<'_> {
        let ui = &self.player_report;
        let phase = if let Some(job) = &ui.job {
            PlayerPhase::Generating(job.control.progress())
        } else if ui.shown.is_some() {
            PlayerPhase::Ready
        } else if let Some(failure) = ui.failure {
            PlayerPhase::Failed(failure)
        } else {
            PlayerPhase::Idle
        };
        let shown = ui.shown.as_ref();
        PlayerReportView {
            open: ui.open,
            phase,
            report: shown.map(|s| &*s.report),
            shared: shown.map(|s| s.report.clone()),
            options: shown.map(|s| &s.options),
            stale: shown.is_some_and(|s| !self.key_is_current(&s.key)),
            prefs: &self.prefs.player_report,
            database_name: shown.map(|s| s.database_name.as_str()),
            confirm_clipbase: ui.confirm.is_some(),
            confirm_overwrite: ui.confirm_save.as_ref().map(|(p, ..)| p.as_path()),
        }
    }

    /// The dialog as it is drawn (`None` when closed).
    pub fn player_report_dialog(&self) -> Option<PlayerDialogView> {
        let request = self.player_report.dialog.clone()?;
        let tags = self.session.game().tags();
        Some(PlayerDialogView {
            request,
            history: self
                .prefs
                .player_tools
                .history
                .iter()
                .take(HISTORY_SHOWN)
                .cloned()
                .collect(),
            white_name: game_name(&tags.white),
            black_name: game_name(&tags.black),
            clipbase_enabled: !self.bases.current().is_clipbase(),
        })
    }

    /// Cancels the job whose database is no longer the current one
    /// (switching, closing or changing the database stops the work). Called
    /// at the end of every `update`.
    pub(crate) fn refresh_player_report(&mut self) {
        let stale_job = self
            .player_report
            .job
            .as_ref()
            .is_some_and(|j| !self.key_is_current(&j.key));
        if stale_job && let Some(job) = self.player_report.job.take() {
            job.control.cancel();
        }
        // The question is about the database it was asked on.
        if self.player_report.confirm.is_some() && self.bases.current().is_clipbase() {
            self.player_report.confirm = None;
        }
    }

    fn prep_prefs(
        &mut self,
        change: impl FnOnce(&mut PlayerReportPrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut next = (*self.prefs.player_report).clone();
        change(&mut next);
        let next = next.sanitized();
        if next != *self.prefs.player_report {
            self.prefs_message(
                PrefsMessage::Set(PrefChange::PlayerReport(Box::new(next))),
                effects,
            );
        }
    }

    /// The dialog's first choices for `player`: the colour comes from the
    /// game on the board when the name is one of its players (spec section 1).
    fn prep_seed(&self, player: Option<&str>) -> PlayerRequest {
        let prefs = &self.prefs.player_report;
        let tags = self.session.game().tags();
        let name = player
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty())
            .or_else(|| self.prefs.player_tools.history.first().cloned())
            .or_else(|| game_name(&tags.white))
            .unwrap_or_default();
        let color = if game_name(&tags.white).as_deref() == Some(name.as_str()) {
            PlayerColor::White
        } else if game_name(&tags.black).as_deref() == Some(name.as_str()) {
            PlayerColor::Black
        } else {
            prefs.last_color
        };
        PlayerRequest {
            name,
            color,
            matching: prefs.matching,
            start: prefs.last_start,
            copy_to_clipbase: prefs.copy_to_clipbase && !self.bases.current().is_clipbase(),
        }
    }

    /// Handles a [`PlayerReportMessage`].
    pub(crate) fn player_report_message(
        &mut self,
        message: PlayerReportMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            PlayerReportMessage::OpenDialog { player } => {
                let slot = self.bases.slot(self.bases.current());
                match slot {
                    None => self.prep_notice(PlayerReportNotice::Unavailable),
                    Some(slot) if slot.total() == 0 => {
                        self.prep_notice(PlayerReportNotice::NoGames);
                    }
                    Some(_) => {
                        let seed = self.prep_seed(player.as_deref());
                        self.player_report.dialog = Some(seed);
                    }
                }
            }
            PlayerReportMessage::CloseDialog => {
                self.player_report.dialog = None;
                self.player_report.confirm = None;
            }
            PlayerReportMessage::Generate(request) => self.prep_generate(*request, false, effects),
            PlayerReportMessage::ConfirmClipbase(yes) => {
                if let Some(request) = self.player_report.confirm.take()
                    && yes
                {
                    self.prep_generate(request, true, effects);
                }
            }
            PlayerReportMessage::ConfirmSave(yes) => {
                if let Some((path, part, format)) = self.player_report.confirm_save.take()
                    && yes
                {
                    self.prep_save_file(&path, part, format, true, effects);
                }
            }
            PlayerReportMessage::Refresh => self.prep_refresh(effects),
            PlayerReportMessage::Stop => {
                if let Some(job) = &self.player_report.job {
                    job.control.cancel();
                }
            }
            PlayerReportMessage::Close => {
                self.player_report.open = false;
                self.player_report.confirm_save = None;
                // A save still in flight is abandoned: its answer is ignored.
                self.player_report.saving = None;
                if let Some(job) = self.player_report.job.take() {
                    job.control.cancel();
                }
            }
            PlayerReportMessage::SetLabels(labels) => self.player_report.labels = *labels,
            PlayerReportMessage::SetPrefs(prefs) => self.prep_prefs(|p| *p = *prefs, effects),
            PlayerReportMessage::ResetPrefs => self.prep_prefs(
                |p| {
                    *p = PlayerReportPrefs {
                        last_color: p.last_color,
                        last_start: p.last_start,
                        matching: p.matching,
                        copy_to_clipbase: p.copy_to_clipbase,
                        last_part: p.last_part,
                        last_format: p.last_format,
                        last_dir: p.last_dir.take(),
                        ..PlayerReportPrefs::default()
                    };
                },
                effects,
            ),
            PlayerReportMessage::Select(selector) => self.prep_select(selector),
            PlayerReportMessage::LoadGame(number) => self.prep_load_game(number, effects),
            PlayerReportMessage::OpenOpponent(name) => {
                self.player_info_message(PlayerInfoMessage::Open { name }, effects);
            }
            PlayerReportMessage::Save { path, part, format } => {
                self.prep_save(&path, part, format, effects);
            }
            PlayerReportMessage::ViewHtml => self.prep_view(effects),
            PlayerReportMessage::Computed { request, outcome } => {
                self.prep_computed(request, *outcome, effects);
            }
            PlayerReportMessage::Saved { request, result } => {
                self.prep_saved(request, result, effects);
            }
            PlayerReportMessage::HtmlWritten { request, result } => {
                if self.player_report.view.pending != Some(request) {
                    return;
                }
                self.player_report.view.pending = None;
                match result {
                    Some((path, dir)) => {
                        self.player_report.view.dir = Some(dir);
                        effects.push(Effect::PlayerReport(PlayerReportEffect::OpenFile { path }));
                    }
                    None => self.prep_notice(PlayerReportNotice::ViewFailed),
                }
            }
            PlayerReportMessage::FileOpened { ok } => {
                if !ok {
                    self.prep_notice(PlayerReportNotice::ViewFailed);
                }
            }
        }
    }

    /// "OK" of the dialog. `confirmed`: the user already answered the
    /// question about the Clipbase.
    fn prep_generate(
        &mut self,
        request: PlayerRequest,
        confirmed: bool,
        effects: &mut Vec<Effect>,
    ) {
        if !usable_name(&request.name) {
            self.prep_notice(PlayerReportNotice::InvalidName);
            return;
        }
        let id = self.bases.current();
        let Some(slot) = self.bases.slot(id).filter(|s| !s.needs_reload) else {
            self.prep_notice(PlayerReportNotice::Unavailable);
            return;
        };
        if slot.total() == 0 {
            self.prep_notice(PlayerReportNotice::NoGames);
            return;
        }
        if request.copy_to_clipbase {
            if id.is_clipbase() {
                self.prep_notice(PlayerReportNotice::ClipbaseSelf);
                return;
            }
            // The Scid empties the Clipbase without asking; the MyChess
            // asks when there is something to lose (M7-3/D3658).
            let has_games = self
                .bases
                .slot(SlotId::CLIPBASE)
                .is_some_and(|c| c.total() > 0);
            if has_games && !confirmed {
                self.player_report.confirm = Some(request);
                return;
            }
        }
        let name = request.name.trim().to_owned();
        let reference = (request.start == PlayerStart::Position).then(|| self.board_reference());
        let params = Params {
            name: name.clone(),
            color: request.color,
            matching: request.matching,
            start: request.start,
            reference,
        };
        // Remember the choices and the name (the history is the one of the
        // player information).
        let (color, start, matching, copy) = (
            request.color,
            request.start,
            request.matching,
            request.copy_to_clipbase,
        );
        self.prep_prefs(
            |p| {
                p.last_color = color;
                p.last_start = start;
                p.matching = matching;
                p.copy_to_clipbase = copy;
            },
            effects,
        );
        let remembered = name;
        self.tools_prefs(move |p| p.remember(&remembered), effects);
        self.player_report.dialog = None;
        self.player_report.confirm = None;
        self.prep_start(params, request.copy_to_clipbase, effects);
    }

    /// "Update": the same choices over the database in use now. The
    /// Clipbase is not emptied again (it would need a new confirmation).
    fn prep_refresh(&mut self, effects: &mut Vec<Effect>) {
        let Some(params) = self.player_report.params.clone() else {
            self.prep_notice(PlayerReportNotice::NoReport);
            return;
        };
        // A position the board moved away from stays the one of the report.
        self.prep_start(params, false, effects);
    }

    fn prep_start(&mut self, params: Params, copy_to_clipbase: bool, effects: &mut Vec<Effect>) {
        if let Some(job) = self.player_report.job.take() {
            job.control.cancel();
        }
        let id = self.bases.current();
        let Some(key) = self.slot_key(id) else {
            self.prep_notice(PlayerReportNotice::Unavailable);
            return;
        };
        let Some(source) = self.bases.slot(id).map(|s| s.source.clone()) else {
            return;
        };
        let request = self.new_request();
        let control = OpenControl::new();
        let options = self.prefs.player_report.options();
        let job = PlayerComputeJob {
            request,
            source,
            name: params.name.clone(),
            color: params.color,
            matching: params.matching,
            reference: params.reference.clone(),
            options: options.clone(),
            stats: (*self.prefs.stats).clone(),
            eco: self.eco_book.clone(),
            player_stats: self.player_stats(id),
            today: self.player_report.today,
            control: control.clone(),
        };
        let database_name = self.base_name(id);
        let ui = &mut self.player_report;
        ui.open = true;
        ui.failure = None;
        ui.params = Some(params);
        ui.job = Some(Job {
            request,
            key,
            options,
            database_name,
            control,
            copy_to_clipbase,
        });
        effects.push(Effect::PlayerReport(PlayerReportEffect::Compute(Box::new(
            job,
        ))));
    }

    fn prep_computed(
        &mut self,
        request: RequestId,
        outcome: PlayerOutcome,
        effects: &mut Vec<Effect>,
    ) {
        if self.player_report.job.as_ref().map(|j| j.request) != Some(request) {
            return;
        }
        let Some(job) = self.player_report.job.take() else {
            return;
        };
        if !self.key_is_current(&job.key) {
            return;
        }
        match outcome {
            PlayerOutcome::Ready { report, generated } => {
                let interrupted = report.core.interrupted;
                let numbers: Vec<u32> = if job.copy_to_clipbase && !interrupted {
                    let mut numbers: Vec<u32> = report.core.lines.iter().map(|l| l.game).collect();
                    numbers.sort_unstable();
                    numbers.dedup();
                    numbers
                } else {
                    Vec::new()
                };
                let slot = job.key.slot;
                self.player_report.shown = Some(Shown {
                    key: job.key,
                    report,
                    options: job.options,
                    generated,
                    database_name: job.database_name,
                });
                if interrupted {
                    self.prep_notice(PlayerReportNotice::Interrupted);
                } else if job.copy_to_clipbase {
                    self.prep_copy_to_clipbase(slot, numbers, effects);
                }
            }
            PlayerOutcome::Failed(failure) => {
                let ui = &mut self.player_report;
                ui.shown = None;
                ui.failure = Some(failure);
                self.prep_notice(match failure {
                    PlayerFailure::InvalidName => PlayerReportNotice::InvalidName,
                    PlayerFailure::UnsupportedPosition => PlayerReportNotice::UnsupportedPosition,
                    PlayerFailure::BadLine => PlayerReportNotice::BadLine,
                    _ => PlayerReportNotice::Failed,
                });
            }
        }
    }

    /// Empties the Clipbase and copies the games of the report to it, with
    /// their order in the database (M7-3/D3658). The question was answered
    /// before the report began.
    fn prep_copy_to_clipbase(
        &mut self,
        from: SlotId,
        numbers: Vec<u32>,
        effects: &mut Vec<Effect>,
    ) {
        // `clipbase_busy` tells the user when a job holds the Clipbase.
        if from.is_clipbase() || self.list_busy() || self.clipbase_busy() {
            return;
        }
        self.clipbase_clear();
        if numbers.is_empty() {
            return;
        }
        self.start_copy_list(
            from,
            SlotId::CLIPBASE,
            CopyList::Numbers(numbers),
            ListJobKind::Copy,
            effects,
        );
    }

    /// The report on the window, if it is there and still matches the
    /// database; otherwise says why not.
    fn prep_fresh(&mut self) -> Option<(SlotId, crate::player_report::SharedPlayerReport)> {
        let Some(shown) = &self.player_report.shown else {
            self.prep_notice(PlayerReportNotice::NoReport);
            return None;
        };
        if !self.key_is_current(&shown.key) {
            self.prep_notice(PlayerReportNotice::Stale);
            return None;
        }
        Some((shown.key.slot, shown.report.clone()))
    }

    /// A click on the text: the games of the selection become the filter of
    /// the database of the report (M7-3/D3653, M7-3/D3661).
    fn prep_select(&mut self, selector: ReportSelector) {
        if self.list_busy() {
            self.prep_notice(PlayerReportNotice::Busy);
            return;
        }
        let Some((id, report)) = self.prep_fresh() else {
            return;
        };
        let Some(slot) = self.bases.slot_mut(id) else {
            return;
        };
        slot.filter = select_games(&report.core, selector.engine(), slot.total());
        slot.filter_changed();
        slot.list.first = 1;
        self.prep_notice(PlayerReportNotice::FilterSet);
    }

    fn prep_load_game(&mut self, number: u32, effects: &mut Vec<Effect>) {
        let Some((id, report)) = self.prep_fresh() else {
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

    fn prep_render_job(&self, part: ReportPart, format: ReportFormat) -> Option<PlayerRenderJob> {
        let shown = self.player_report.shown.as_ref()?;
        Some(PlayerRenderJob {
            report: shown.report.clone(),
            options: shown.options.clone(),
            labels: Box::new(self.player_report.labels.clone()),
            part,
            format,
            database_name: shown.database_name.clone(),
            generated: shown.generated,
        })
    }

    fn prep_save(
        &mut self,
        path: &Path,
        part: ReportPart,
        format: ReportFormat,
        effects: &mut Vec<Effect>,
    ) {
        // The file dialog confirmed `path`; the extension may change it.
        self.prep_save_file(path, part, format, false, effects);
    }

    fn prep_save_file(
        &mut self,
        chosen: &Path,
        part: ReportPart,
        format: ReportFormat,
        confirmed: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.player_report.saving.is_some() {
            self.prep_notice(PlayerReportNotice::Busy);
            return;
        }
        let Some(render) = self.prep_render_job(part, format) else {
            self.prep_notice(PlayerReportNotice::NoReport);
            return;
        };
        let path = with_report_extension(chosen, format);
        let confirmed = confirmed || path == chosen;
        let request = self.new_request();
        self.player_report.saving = Some((request, part, format));
        effects.push(Effect::PlayerReport(PlayerReportEffect::Save(Box::new(
            PlayerSaveJob {
                request,
                path,
                confirmed,
                render,
                protected: self.orep_protected(),
            },
        ))));
    }

    fn prep_saved(
        &mut self,
        request: RequestId,
        result: Result<PathBuf, SaveFailure>,
        effects: &mut Vec<Effect>,
    ) {
        let Some((pending, part, format)) = self.player_report.saving else {
            return;
        };
        if pending != request {
            return;
        }
        self.player_report.saving = None;
        match result {
            Ok(path) => {
                let dir = path
                    .parent()
                    .map(Path::to_path_buf)
                    .filter(|d| !d.as_os_str().is_empty());
                self.prep_prefs(
                    |p| {
                        p.last_part = part;
                        p.last_format = format;
                        if dir.is_some() {
                            p.last_dir = dir;
                        }
                    },
                    effects,
                );
                self.prep_notice(PlayerReportNotice::Saved);
            }
            Err(SaveFailure::ProtectedFile) => self.prep_notice(PlayerReportNotice::SaveProtected),
            Err(SaveFailure::NeedsConfirm(path)) => {
                self.player_report.confirm_save = Some((path, part, format));
            }
            Err(_) => self.prep_notice(PlayerReportNotice::SaveFailed),
        }
    }

    fn prep_view(&mut self, effects: &mut Vec<Effect>) {
        let Some(html) = self.prep_render_job(ReportPart::Full, ReportFormat::Html) else {
            self.prep_notice(PlayerReportNotice::NoReport);
            return;
        };
        let request = self.new_request();
        self.player_report.view.pending = Some(request);
        effects.push(Effect::PlayerReport(PlayerReportEffect::WriteHtml {
            request,
            html: Box::new(html),
            dir: self.player_report.view.dir.clone(),
        }));
    }
}
