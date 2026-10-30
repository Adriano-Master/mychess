//! The glue of the spelling file, the name checker and "Add Elo" with
//! [`AppState`] (spec M6-3, slice b).
//!
//! Everything here is bookkeeping, which `update` may do. The reading of the
//! file, the scans and the writes are the effects of [`SpellingEffect`]. The
//! two writes (names and ratings) run as jobs of the list (kind
//! [`ListJobKind::SpellApply`] and [`ListJobKind::EloAdd`], D3462), so closing the slot, quitting and the
//! protection of the file against a second open all work as they do for the
//! other writes; the answer comes as a [`SpellingMessage`].

use std::path::PathBuf;

use mychess_db::GameSource;

use super::AppState;
use crate::bases::{BaseKind, ListColumn, ListJobKind, OpenControl, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::AppNotice;
use crate::game_info::NameKind;
use crate::host::{PickKind, PickSpec};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::spelling_ui::{
    AddEloDialog, AddEloFlags, AddEloView, CheckDialog, EloOutcome, NamesJob, NamesOutcome,
    PlannedElo, PlayerToolsPrefs, Running, ScanDone, Shared, SpellFlags, SpellView, SpellingEffect,
    SpellingMessage, SpellingNotice, SpellingStatusView, StatsEntry, StatsRun, Target, ToolsDelta,
};
use crate::spelling_ui::{DeferredWrite, LoadRun, PlanJob, ScanJob};

/// What a write left behind, to be put in the slot.
struct Reread {
    slot: SlotId,
    /// The database read again (`.si4` only).
    source: Option<crate::bases::SharedSource>,
    stamp: Option<crate::bases::FileStamp>,
    /// The change was made in place (the Clipbase).
    in_memory: bool,
    /// Some game changed.
    changed: bool,
    /// The names changed (the tags of the game on screen are stale).
    names: bool,
}

impl AppState {
    /// The state of the spelling file and its windows.
    pub fn spelling(&self) -> &crate::spelling_ui::SpellingUi {
        &self.spelling
    }

    /// Sets the data directory where `spelling.ssp` and `photos/` are looked
    /// for (`None`: the system's). For tests and the host.
    pub fn set_player_data_dir(&mut self, dir: Option<PathBuf>) {
        self.spelling.data_dir = dir;
    }

    /// The highest Elo of every player of a slot, with the estimates of the
    /// spelling file (spec rule 87, D3338); `None` while it is being built,
    /// for the Clipbase, and when no spelling file is loaded (a consumer
    /// then counts the games itself).
    pub fn player_stats(&self, slot: SlotId) -> Option<Shared<mychess_db::PlayerStats>> {
        let set = self.spelling.set.as_ref()?;
        let fingerprint = set.checker(mychess_db::si4::NameKind::Player).fingerprint();
        let base = self.bases.slot(slot)?;
        self.spelling
            .stats
            .iter()
            .find(|e| {
                e.slot == slot
                    && e.source == base.source
                    && e.len == base.total()
                    && e.fingerprint == fingerprint
            })
            .map(|e| e.stats.clone())
    }

    fn spelling_notice(&mut self, notice: SpellingNotice) {
        self.notice = Some(AppNotice::Spelling(notice));
    }

    /// Changes the `[player_tools]` preferences and saves them.
    ///
    /// While the preferences file is being read the change is applied to the
    /// values on show and kept as a delta, which `preferences_loaded` applies
    /// again over the file's values: sending the whole table would replace
    /// the history, the spelling file and the rest of the user's file.
    pub(crate) fn tools_prefs(
        &mut self,
        change: impl Fn(&mut PlayerToolsPrefs) + Send + Sync + 'static,
        effects: &mut Vec<Effect>,
    ) {
        let mut prefs = (*self.prefs.player_tools).clone();
        change(&mut prefs);
        let prefs = prefs.sanitized();
        if self.flow.prefs_loading {
            *self.prefs.player_tools = prefs;
            self.spelling
                .early_tools
                .push(ToolsDelta(std::sync::Arc::new(change)));
            // Waits for `PreferencesLoaded` to save.
            self.request_prefs_save(effects);
            return;
        }
        if prefs == *self.prefs.player_tools {
            return;
        }
        self.prefs_message(
            PrefsMessage::Set(PrefChange::PlayerTools(Box::new(prefs))),
            effects,
        );
    }

    /// Whether the names or ratings of `slot` are being written: its file
    /// must not be read meanwhile (Windows refuses, the others could read a
    /// half-written file).
    pub(crate) fn slot_rewriting(&self, slot: SlotId) -> bool {
        self.flow.list_job.as_ref().is_some_and(|j| {
            matches!(j.kind, ListJobKind::SpellApply | ListJobKind::EloAdd)
                && (j.slot == slot || j.target.as_ref().is_some_and(|(to, _)| *to == slot))
        }) || self.maintenance_rewriting(slot)
    }

    /// Whether the write has to wait for the build of statistics of `slot`,
    /// which it stops here and which ends on its own soon: the thread must
    /// let go of the database before its files are rewritten.
    fn defer_for_reads(&mut self, slot: SlotId, what: DeferredWrite) -> bool {
        if let Some(run) = self.spelling.stats_run.take_if(|r| r.slot == slot) {
            run.run.control.cancel();
            self.spelling.stats_draining = Some(run.run.request);
        }
        if self.spelling.stats_draining.is_none() {
            return false;
        }
        self.spelling.write_deferred = Some(what);
        true
    }

    /// Stops the reads of the database of `slot`, which a write is about to
    /// rewrite; `reads_resume` makes them start again.
    pub(super) fn reads_pause(&mut self, slot: SlotId) {
        // The cancelled answer is dropped (its request no longer matches):
        // it must not mark the key as stopped after `reads_resume`.
        if self
            .spelling
            .stats_run
            .as_ref()
            .is_some_and(|r| r.slot == slot)
            && let Some(run) = self.spelling.stats_run.take()
        {
            run.run.control.cancel();
        }
        if let Some(scan) = self
            .spelling
            .check
            .as_ref()
            .filter(|d| d.target.slot == slot)
            .and_then(|d| d.scan.as_ref())
        {
            scan.control.cancel();
        }
        if let Some((run, _)) = self
            .spelling
            .add_elo
            .as_ref()
            .filter(|d| d.slot == slot)
            .and_then(|d| d.planning.as_ref())
        {
            run.control.cancel();
        }
        if self.bases.current() == slot {
            self.finder.players.refresh.cancel();
            self.finder.tournaments.refresh.cancel();
            self.player_info.refresh.cancel();
        }
    }

    /// Lets the windows that follow the database compute again (the write
    /// ended, or its answer is stale and the slot is unknown).
    pub(super) fn reads_unblock(&mut self) {
        if self.spelling.quitting {
            return;
        }
        self.spelling.stats_stopped = None;
        self.finder.players.refresh.force();
        self.finder.tournaments.refresh.force();
        self.player_info.refresh.force();
    }

    /// After a write: the windows that follow the database compute again,
    /// and so does the name checker (`rescan`) if the write cut its scan.
    fn reads_resume(&mut self, slot: SlotId, rescan: bool, effects: &mut Vec<Effect>) {
        if self.spelling.quitting {
            return;
        }
        self.reads_unblock();
        // The options asked for during the write count too (D3496).
        let cut = self.spelling.check.as_ref().is_some_and(|d| {
            d.target.slot == slot
                && (d.options_changed
                    || (rescan
                        && (d.interrupted
                            || d.scan.as_ref().is_some_and(|s| s.control.is_cancelled()))))
        });
        if cut {
            self.spell_scan(effects);
        }
    }

    /// Stops what runs (the program is ending).
    pub(crate) fn spelling_cancel(&mut self) {
        self.spelling.quitting = true;
        self.spelling.cancel();
    }

    /// The quit did not end the program (the question about unsaved changes
    /// was cancelled): what `spelling_cancel` stopped starts again (D3494).
    pub(crate) fn spelling_quit_undone(&mut self, effects: &mut Vec<Effect>) {
        if !self.spelling.quitting {
            return;
        }
        self.spelling.quitting = false;
        self.spelling.stats_stopped = None;
        // What the maintenance tools stopped starts again (spec M7-1).
        self.maintenance_quit_undone();
        // The cancelled build is dropped, so its late answer cannot mark the
        // key as stopped (D3503).
        // The cancelled thread goes through `stats_draining`: it still holds
        // the files until it answers, so neither a write nor a new build
        // starts before (Windows, D3510). Its answer starts them.
        if let Some(run) = self
            .spelling
            .stats_run
            .take_if(|r| r.run.control.is_cancelled())
        {
            self.spelling.stats_draining = Some(run.run.request);
        }
        self.finder.players.refresh.force();
        self.finder.tournaments.refresh.force();
        self.player_info.refresh.force();
        // The load the quit cut starts again: either its answer already came
        // (`cut_load`, the usual order: the thread ends in milliseconds,
        // before the user answers the question) or it is still running.
        let cut_load = self.spelling.cut_load.take().or_else(|| {
            self.spelling
                .load
                .take_if(|l| l.run.control.is_cancelled() && !l.user_cancelled)
        });
        let photos_cut = std::mem::take(&mut self.spelling.photos_cut)
            || self
                .spelling
                .photos_run
                .as_ref()
                .is_some_and(|r| r.control.is_cancelled());
        if let Some(load) = cut_load {
            // The photos follow the load.
            self.spelling_start_load(load.path, load.remember, load.quiet, effects);
        } else if photos_cut && self.spelling.set.is_some() {
            self.spelling_start_photos(effects);
        }
        let cut = self.spelling.check.as_ref().is_some_and(|d| {
            d.interrupted || d.scan.as_ref().is_some_and(|s| s.control.is_cancelled())
        });
        if cut {
            self.spell_scan(effects);
        }
        // The write that waited for the build to end (D3504).
        if self.spelling.stats_draining.is_none() {
            self.run_deferred_write(effects);
        }
    }

    /// Starts the write that waited for the build of statistics to end.
    fn run_deferred_write(&mut self, effects: &mut Vec<Effect>) {
        match self.spelling.write_deferred.take() {
            Some(DeferredWrite::Names) => self.spell_write(effects),
            Some(DeferredWrite::Elo) => self.elo_write(effects),
            None => {}
        }
    }

    /// The first spelling command after a quit that did not end the program
    /// and told nothing to the spelling flow (the file dialog of "Save" was
    /// cancelled, the save failed...): the quit is undone here (D3505). Not
    /// while the quit is still going on: a question open or any wait of
    /// `finish_quit` (D3509, `quit_finishing`).
    fn spelling_heal_quit(&mut self, message: &SpellingMessage, effects: &mut Vec<Effect>) {
        if self.spelling.quitting && self.quit_not_going_on() && spelling_is_user_command(message) {
            self.spelling_quit_undone(effects);
        }
    }

    /// Whether no part of a quit is going on: no question open and no wait
    /// of `finish_quit` (D3509). The safety nets of the flows that a quit
    /// stops use it to know that the quit was given up without telling them.
    pub(super) fn quit_not_going_on(&self) -> bool {
        !self.quit_finishing()
            && self.discard_prompt.is_none()
            && self.overwrite_prompt.is_none()
            && self.flow.pick.is_none()
            && self.flow.save.is_none()
            && self.tree.mask.prompt.is_none()
            && self.tree.mask.pick.is_none()
            && self.tree.mask.writing.is_none()
    }

    // ---- views ----

    /// What the status of the spelling file shows.
    pub fn spelling_status_view(&self) -> SpellingStatusView {
        let ui = &self.spelling;
        SpellingStatusView {
            loaded: ui.set.is_some(),
            path: ui.path.clone(),
            counts: ui.counts,
            loading: ui.load.as_ref().map(|l| l.run.control.progress()),
            photos: ui.photo_counts,
            show_photos: self.prefs.player_tools.show_photos,
        }
    }

    fn rebuild_check_rows(&mut self) {
        if let Some(dialog) = self.spelling.check.as_mut() {
            dialog.rebuild_rows();
        }
    }

    fn write_progress(&self) -> Option<(u64, u64)> {
        self.spelling.write?;
        self.list_job()
            .filter(|(kind, _)| matches!(kind, ListJobKind::SpellApply | ListJobKind::EloAdd))
            .map(|(_, control)| control.progress())
    }

    fn slot_writable(&self, slot: SlotId) -> bool {
        self.bases
            .slot(slot)
            .is_some_and(|s| !s.read_only && s.kind != BaseKind::Pgn && !s.needs_reload)
    }

    /// The name checker window, when open.
    pub fn spell_view(&self) -> Option<SpellView> {
        let dialog = self.spelling.check.as_ref()?;
        Some(SpellView {
            kind: dialog.kind,
            rows: dialog.rows_view.clone(),
            total: dialog.rows.len(),
            ticked: dialog.ticked,
            find: dialog.find.clone(),
            surnames: dialog.surnames,
            ambiguous: dialog.ambiguous,
            scanning: dialog.scan.as_ref().map(|s| s.control.progress()),
            scanned: dialog.scanned,
            interrupted: dialog.interrupted,
            confirm: dialog.confirm,
            writing: self.write_progress(),
            writable: self.slot_writable(dialog.target.slot),
            in_clipbase: dialog.target.slot.is_clipbase(),
            result: dialog.result.clone(),
        })
    }

    /// The flags of the name checker (cheap: nothing is cloned), when open.
    pub fn spell_flags(&self) -> Option<SpellFlags> {
        let dialog = self.spelling.check.as_ref()?;
        Some(SpellFlags {
            scanning: dialog.scan.is_some(),
            confirm: dialog.confirm,
            writing: self.write_progress().is_some(),
            rows: dialog.rows_view.len(),
        })
    }

    /// The flags of the "Add Elo" dialog (cheap), when open.
    pub fn add_elo_flags(&self) -> Option<AddEloFlags> {
        let dialog = self.spelling.add_elo.as_ref()?;
        Some(AddEloFlags {
            planning: dialog.planning.is_some(),
            planned: dialog.planned.is_some(),
            writing: dialog.writing.is_some() && self.write_progress().is_some(),
            has_result: dialog.result.is_some(),
        })
    }

    /// The "Add Elo ratings" dialog, when open.
    pub fn add_elo_view(&self) -> Option<AddEloView> {
        let dialog = self.spelling.add_elo.as_ref()?;
        Some(AddEloView {
            overwrite: dialog.overwrite,
            only_filter: dialog.only_filter,
            planning: dialog
                .planning
                .as_ref()
                .map(|(run, _)| run.control.progress()),
            planned: dialog
                .planned
                .as_ref()
                .map(|(_, p)| (p.report.ratings, p.report.games)),
            writing: if dialog.writing.is_some() {
                self.write_progress()
            } else {
                None
            },
            writable: self.slot_writable(dialog.slot),
            result: dialog.result.clone(),
        })
    }

    // ---- loading ----

    fn spelling_start_load(
        &mut self,
        path: Option<PathBuf>,
        remember: Option<PathBuf>,
        quiet: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.spelling.quitting {
            return;
        }
        if let Some(old) = self.spelling.load.take() {
            old.run.control.cancel();
        }
        let request = self.new_request();
        let control = OpenControl::new();
        self.spelling.load = Some(LoadRun {
            path: path.clone(),
            run: Running {
                request,
                control: control.clone(),
            },
            remember,
            quiet,
            user_cancelled: false,
        });
        effects.push(Effect::Spelling(SpellingEffect::Load {
            request,
            path,
            data_dir: self.spelling.data_dir.clone(),
            quiet,
            control,
        }));
    }

    fn spelling_start_photos(&mut self, effects: &mut Vec<Effect>) {
        if let Some(old) = self.spelling.photos_run.take() {
            old.control.cancel();
        }
        let request = self.new_request();
        let control = OpenControl::new();
        self.spelling.photos_run = Some(Running {
            request,
            control: control.clone(),
        });
        let extra = Some(self.prefs.player_tools.photos_dir.clone())
            .filter(|d| !d.is_empty())
            .map(PathBuf::from);
        effects.push(Effect::Spelling(SpellingEffect::Photos {
            request,
            extra,
            data_dir: self.spelling.data_dir.clone(),
            control,
        }));
    }

    /// The answer of the dialog of `PickFile`.
    pub(crate) fn spelling_file_picked(
        &mut self,
        request: RequestId,
        path: Option<PathBuf>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if self.spelling.pick != Some(request) {
            return false;
        }
        self.spelling.pick = None;
        if let Some(path) = path {
            self.spelling_message(SpellingMessage::Load(path), effects);
        }
        true
    }

    fn spelling_pick(&mut self, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        self.spelling.pick = Some(request);
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind: PickKind::OpenSpelling,
            initial_dir: self.prefs.files.last_open_dir.clone(),
            file_name: None,
        }));
    }

    fn spelling_loaded(
        &mut self,
        request: RequestId,
        result: Result<crate::spelling_ui::LoadedSpelling, crate::spelling_ui::SpellingLoadError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.spelling.load.as_ref().map(|l| l.run.request) != Some(request) {
            return;
        }
        let Some(load) = self.spelling.load.take() else {
            return;
        };
        match result {
            Ok(loaded) => {
                self.spelling.set = Some(loaded.set);
                self.spelling.path = Some(loaded.path);
                self.spelling.counts = Some(loaded.counts);
                self.spelling_notice(SpellingNotice::Loaded(loaded.counts));
                if let Some(path) = load.remember {
                    let text = path.to_string_lossy().into_owned();
                    self.tools_prefs(move |p| p.spelling_file.clone_from(&text), effects);
                }
                if self.spelling.quitting {
                    // A quit given up starts them (D3502).
                    self.spelling.photos_cut = true;
                } else {
                    self.spelling_start_photos(effects);
                }
            }
            Err(crate::spelling_ui::SpellingLoadError::Cancelled) => {
                // The quit cut the load (the answer comes before the user
                // answers the question): a quit given up starts it again
                // (D3502). A load the user cancelled does not.
                if self.spelling.quitting && !load.user_cancelled {
                    self.spelling.cut_load = Some(load);
                }
            }
            Err(crate::spelling_ui::SpellingLoadError::File(
                crate::error::FileErrorKind::NotFound,
            )) if load.quiet => {}
            Err(error) => self.spelling_notice(SpellingNotice::LoadFailed(error)),
        }
    }

    // ---- the cache of PlayerStats ----

    /// Builds the `PlayerStats` of the slots that need them (D3338): called
    /// at the end of every `update`. Does nothing without a spelling file
    /// or for the Clipbase.
    pub(crate) fn refresh_spelling(&mut self, effects: &mut Vec<Effect>) {
        let Some(set) = self.spelling.set.clone() else {
            self.spelling.stats.clear();
            return;
        };
        let fingerprint = set.checker(mychess_db::si4::NameKind::Player).fingerprint();
        let slots: Vec<(SlotId, crate::bases::SharedSource, u32)> = self
            .bases
            .iter()
            .filter(|(_, s)| s.kind != BaseKind::Clipbase && !s.needs_reload)
            .map(|(id, s)| (id, s.source.clone(), s.total()))
            .collect();
        self.spelling.stats.retain(|e| {
            e.fingerprint == fingerprint
                && slots
                    .iter()
                    .any(|(id, source, len)| *id == e.slot && *source == e.source && *len == e.len)
        });
        if self.spelling.stats_run.is_some()
            || self.spelling.stats_draining.is_some()
            || self.spelling.quitting
        {
            return;
        }
        let next = slots.into_iter().find(|(id, source, len)| {
            let key = (*id, source.clone(), *len, fingerprint);
            !self.slot_rewriting(*id)
                && !self.spelling.stats.iter().any(|e| {
                    (e.slot, &e.source, e.len, e.fingerprint) == (key.0, &key.1, key.2, key.3)
                })
                && self.spelling.stats_stopped.as_ref() != Some(&key)
        });
        let Some((slot, source, len)) = next else {
            return;
        };
        let request = self.new_request();
        let control = OpenControl::new();
        self.spelling.stats_run = Some(StatsRun {
            run: Running {
                request,
                control: control.clone(),
            },
            slot,
            source: source.clone(),
            len,
            fingerprint,
        });
        effects.push(Effect::Spelling(SpellingEffect::Stats {
            request,
            source,
            set,
            control,
        }));
    }

    fn spelling_stats_built(
        &mut self,
        request: RequestId,
        stats: Option<Shared<mychess_db::PlayerStats>>,
        effects: &mut Vec<Effect>,
    ) {
        // The build a write stopped is over: its files are free, so the
        // write may start (D3498).
        if self.spelling.stats_draining == Some(request) {
            self.spelling.stats_draining = None;
            // During a quit the write stays deferred: a quit given up runs it
            // (D3504).
            if !self.spelling.quitting {
                self.run_deferred_write(effects);
            }
            return;
        }
        if self.spelling.stats_run.as_ref().map(|s| s.run.request) != Some(request) {
            return;
        }
        let Some(run) = self.spelling.stats_run.take() else {
            return;
        };
        match stats {
            Some(stats) => {
                self.spelling.stats.retain(|e| e.slot != run.slot);
                self.spelling.stats.push(StatsEntry {
                    slot: run.slot,
                    source: run.source,
                    len: run.len,
                    fingerprint: run.fingerprint,
                    stats,
                });
            }
            None => {
                self.spelling.stats_stopped =
                    Some((run.slot, run.source, run.len, run.fingerprint));
            }
        }
    }

    // ---- the messages ----

    /// Handles a [`SpellingMessage`].
    pub(crate) fn spelling_message(&mut self, message: SpellingMessage, effects: &mut Vec<Effect>) {
        self.spelling_heal_quit(&message, effects);
        self.spelling_message_inner(message, effects);
        // A quit waiting for the write goes on when the write is done
        // (D589), as for the other list jobs.
        if self.flow.quit_waiting_job && self.flow.list_job.is_none() {
            self.flow.quit_waiting_job = false;
            self.finish_quit(effects);
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the messages
    fn spelling_message_inner(&mut self, message: SpellingMessage, effects: &mut Vec<Effect>) {
        match message {
            SpellingMessage::LoadAtStart => {
                if self.prefs.player_tools.load_at_start && self.spelling.load.is_none() {
                    let file = self.prefs.player_tools.spelling_file.clone();
                    let path = (!file.is_empty()).then(|| PathBuf::from(file));
                    self.spelling_start_load(path, None, true, effects);
                }
            }
            SpellingMessage::PickFile => self.spelling_pick(effects),
            SpellingMessage::Load(path) => {
                self.spelling_start_load(Some(path.clone()), Some(path), false, effects);
            }
            SpellingMessage::Reload => match self.spelling.path.clone() {
                Some(path) => self.spelling_start_load(Some(path), None, false, effects),
                None => self.spelling_notice(SpellingNotice::NotLoaded),
            },
            SpellingMessage::Unload => {
                if let Some(load) = self.spelling.load.take() {
                    load.run.control.cancel();
                }
                self.spelling.cut_load = None;
                self.spelling.photos_cut = false;
                self.spelling.set = None;
                self.spelling.path = None;
                self.spelling.counts = None;
                self.spelling.stats.clear();
                if let Some(run) = self.spelling.stats_run.take() {
                    run.run.control.cancel();
                }
                if let Some(run) = self.spelling.photos_run.take() {
                    run.control.cancel();
                }
            }
            SpellingMessage::CancelLoad => {
                if let Some(load) = self.spelling.load.as_mut() {
                    load.user_cancelled = true;
                    load.run.control.cancel();
                }
            }
            SpellingMessage::Loaded { request, result } => {
                self.spelling_loaded(request, *result, effects);
            }
            SpellingMessage::PhotosLoaded {
                request,
                library,
                counts,
            } => {
                if self.spelling.photos_run.as_ref().map(|r| r.request) == Some(request) {
                    // Cut by the quit: a quit given up starts them again
                    // (D3502).
                    let cut = self
                        .spelling
                        .photos_run
                        .as_ref()
                        .is_some_and(|r| r.control.is_cancelled());
                    self.spelling.photos_cut = cut && self.spelling.quitting;
                    self.spelling.photos_run = None;
                    self.spelling.photos = library;
                    self.spelling.photo_counts = Some(counts);
                }
            }
            SpellingMessage::StatsBuilt { request, stats } => {
                self.spelling_stats_built(request, stats, effects);
            }
            SpellingMessage::OpenCheck(kind) => self.spell_open(kind, effects),
            SpellingMessage::CloseCheck => {
                if self.spelling.write_deferred == Some(DeferredWrite::Names) {
                    self.spelling.write_deferred = None;
                }
                if let Some(dialog) = self.spelling.check.take()
                    && let Some(scan) = dialog.scan
                {
                    scan.control.cancel();
                }
            }
            SpellingMessage::Rescan => self.spell_scan(effects),
            SpellingMessage::SetSurnames(on) => {
                if let Some(dialog) = self.spelling.check.as_mut() {
                    dialog.surnames = on;
                    dialog.options_changed = true;
                }
                self.tools_prefs(move |p| p.spellcheck.surnames = on, effects);
                self.spell_scan(effects);
            }
            SpellingMessage::SetAmbiguous(on) => {
                if let Some(dialog) = self.spelling.check.as_mut() {
                    dialog.ambiguous = on;
                    dialog.options_changed = true;
                }
                self.tools_prefs(move |p| p.spellcheck.ambiguous = on, effects);
                self.spell_scan(effects);
            }
            SpellingMessage::CancelScan => {
                if let Some(scan) = self.spelling.check.as_ref().and_then(|d| d.scan.as_ref()) {
                    scan.control.cancel();
                }
            }
            SpellingMessage::ToggleRow(index) => {
                if let Some(row) = self
                    .spelling
                    .check
                    .as_mut()
                    .and_then(|d| d.rows.get_mut(index))
                {
                    row.selected = !row.selected;
                }
                self.rebuild_check_rows();
            }
            SpellingMessage::SelectAll(on) => {
                if let Some(dialog) = self.spelling.check.as_mut() {
                    for row in dialog.rows.iter_mut().filter(|r| !r.ambiguous) {
                        row.selected = on;
                    }
                }
                self.rebuild_check_rows();
            }
            SpellingMessage::EditNew(index, text) => {
                if let Some(row) = self
                    .spelling
                    .check
                    .as_mut()
                    .and_then(|d| d.rows.get_mut(index))
                {
                    row.new = text;
                }
                self.rebuild_check_rows();
            }
            SpellingMessage::SetFind(text) => {
                if let Some(dialog) = self.spelling.check.as_mut() {
                    dialog.find = text;
                }
                self.rebuild_check_rows();
            }
            SpellingMessage::CopyText => {
                if let Some(dialog) = &self.spelling.check {
                    let text = crate::spelling_ui::rows_text(dialog.kind, &dialog.rows);
                    effects.push(Effect::WriteClipboard(text));
                }
            }
            SpellingMessage::Apply => self.spell_apply(effects),
            SpellingMessage::ConfirmApply(yes) => {
                // Only an open confirmation can be answered.
                let asked = self
                    .spelling
                    .check
                    .as_mut()
                    .is_some_and(|d| std::mem::take(&mut d.confirm));
                if yes && asked {
                    self.spell_write(effects);
                }
            }
            SpellingMessage::Scanned { request, result } => self.spell_scanned(request, *result),
            SpellingMessage::Applied { request, result } => {
                self.spell_applied(request, *result, effects);
            }
            SpellingMessage::OpenAddElo => self.elo_open(),
            SpellingMessage::CloseAddElo => {
                if self.spelling.write_deferred == Some(DeferredWrite::Elo) {
                    self.spelling.write_deferred = None;
                }
                if let Some(dialog) = self.spelling.add_elo.take()
                    && let Some((run, _)) = dialog.planning
                {
                    run.control.cancel();
                }
            }
            SpellingMessage::SetEloOverwrite(on) => {
                if let Some(dialog) = self.spelling.add_elo.as_mut() {
                    dialog.overwrite = on;
                    dialog.planned = None;
                }
                self.tools_prefs(move |p| p.add_elo.overwrite = on, effects);
            }
            SpellingMessage::SetEloOnlyFilter(on) => {
                if let Some(dialog) = self.spelling.add_elo.as_mut() {
                    dialog.only_filter = on;
                    dialog.planned = None;
                }
                self.tools_prefs(move |p| p.add_elo.only_filter = on, effects);
            }
            SpellingMessage::PlanElo => self.elo_plan(effects),
            SpellingMessage::ConfirmElo => self.elo_write(effects),
            SpellingMessage::Planned { request, result } => self.elo_planned(request, *result),
            SpellingMessage::EloApplied { request, result } => {
                self.elo_applied(request, *result, effects);
            }
            SpellingMessage::CancelJob => {
                if let Some((run, _)) = self
                    .spelling
                    .add_elo
                    .as_ref()
                    .and_then(|d| d.planning.as_ref())
                {
                    run.control.cancel();
                }
                if self.spelling.write.is_some()
                    && let Some(job) = &self.flow.list_job
                {
                    job.control.cancel();
                }
            }
        }
    }

    // ---- the name checker ----

    fn spell_open(&mut self, kind: NameKind, effects: &mut Vec<Effect>) {
        if self.spelling.set.is_none() {
            // The Scid opens the file dialog when the file is missing.
            self.spelling_notice(SpellingNotice::NotLoaded);
            self.spelling_pick(effects);
            return;
        }
        let id = self.bases.current();
        let Some(slot) = self.bases.slot(id) else {
            self.spelling_notice(SpellingNotice::NoBase);
            return;
        };
        // A dialog that is writing stays (the write belongs to it); any
        // other is replaced and its scan stops (D3495).
        if self.spelling.check.is_some() && self.spelling.write.is_some() {
            return;
        }
        // Only the write of the dialog that is replaced is dropped (D3507).
        if self.spelling.write_deferred == Some(DeferredWrite::Names) {
            self.spelling.write_deferred = None;
        }
        if let Some(scan) = self.spelling.check.take().and_then(|old| old.scan) {
            scan.control.cancel();
        }
        let prefs = self.prefs.player_tools.spellcheck;
        self.spelling.check = Some(CheckDialog {
            kind,
            target: Target {
                slot: id,
                source: slot.source.clone(),
                len: slot.total(),
                version: self.spelling.version,
            },
            surnames: prefs.surnames,
            ambiguous: prefs.ambiguous,
            rows: Vec::new(),
            rows_view: Shared::new(Vec::new()),
            ticked: 0,
            scanned: false,
            interrupted: false,
            options_changed: false,
            scan: None,
            find: String::new(),
            confirm: false,
            result: None,
        });
        self.spell_scan(effects);
    }

    fn spell_scan(&mut self, effects: &mut Vec<Effect>) {
        let Some(set) = self.spelling.set.clone() else {
            return;
        };
        // Not while the file is being rewritten; the end of the write scans.
        if self
            .spelling
            .check
            .as_ref()
            .is_some_and(|d| self.slot_rewriting(d.target.slot))
        {
            return;
        }
        let request = self.new_request();
        let Some(dialog) = self.spelling.check.as_mut() else {
            return;
        };
        if let Some(old) = dialog.scan.take() {
            old.control.cancel();
        }
        let Some(slot) = self.bases.slot(dialog.target.slot) else {
            return;
        };
        dialog.target = Target {
            slot: dialog.target.slot,
            source: slot.source.clone(),
            len: slot.total(),
            version: self.spelling.version,
        };
        dialog.rows.clear();
        dialog.rebuild_rows();
        dialog.scanned = false;
        dialog.interrupted = false;
        dialog.options_changed = false;
        dialog.confirm = false;
        dialog.result = None;
        let control = OpenControl::new();
        dialog.scan = Some(Running {
            request,
            control: control.clone(),
        });
        let job = ScanJob {
            source: dialog.target.source.clone(),
            set,
            kind: dialog.kind,
            surnames: dialog.surnames,
            ambiguous: dialog.ambiguous,
        };
        effects.push(Effect::Spelling(SpellingEffect::Scan {
            request,
            job: Box::new(job),
            control,
        }));
    }

    fn spell_scanned(&mut self, request: RequestId, result: ScanDone) {
        let current = self
            .spelling
            .check
            .as_ref()
            .and_then(|d| d.scan.as_ref())
            .map(|s| s.request);
        if current != Some(request) {
            return;
        }
        let Some(dialog) = self.spelling.check.as_mut() else {
            return;
        };
        dialog.scan = None;
        dialog.scanned = true;
        dialog.interrupted = result.interrupted;
        dialog.rows = result.rows;
        dialog.rebuild_rows();
        if result.interrupted {
            self.spelling_notice(SpellingNotice::ScanInterrupted);
        }
    }

    /// Why the database of `target` cannot be written now, if it cannot.
    fn spell_refusal(&self, target: &Target) -> Option<SpellingNotice> {
        let Some(slot) = self.bases.slot(target.slot) else {
            return Some(SpellingNotice::NoBase);
        };
        if slot.source != target.source || self.spelling.version != target.version {
            return Some(SpellingNotice::Stale);
        }
        if slot.read_only || slot.kind == BaseKind::Pgn || slot.needs_reload {
            return Some(SpellingNotice::ReadOnly);
        }
        None
    }

    fn spell_apply(&mut self, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.spelling.check.as_ref() else {
            return;
        };
        if dialog.scan.is_some() || dialog.confirm {
            return;
        }
        let target = dialog.target.clone();
        if crate::spelling_ui::changes_of(&dialog.rows).is_empty() {
            self.spelling_notice(SpellingNotice::NoneSelected);
            return;
        }
        if let Some(notice) = self.spell_refusal(&target) {
            self.spelling_notice(notice);
            return;
        }
        if self.list_busy() {
            return;
        }
        if target.slot.is_clipbase() {
            self.spell_write(effects);
        } else if let Some(dialog) = self.spelling.check.as_mut() {
            dialog.confirm = true;
        }
    }

    fn spell_write(&mut self, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.spelling.check.as_ref() else {
            return;
        };
        let (target, kind) = (dialog.target.clone(), dialog.kind);
        let changes = crate::spelling_ui::changes_of(&dialog.rows);
        if changes.is_empty() {
            self.spelling_notice(SpellingNotice::NoneSelected);
            return;
        }
        if let Some(notice) = self.spell_refusal(&target) {
            self.spelling_notice(notice);
            return;
        }
        if self.list_busy() {
            return;
        }
        if self.defer_for_reads(target.slot, DeferredWrite::Names) {
            return;
        }
        let Some(slot) = self.bases.slot(target.slot) else {
            return;
        };
        let job = NamesJob {
            source: slot.source.clone(),
            path: slot.path.clone().filter(|_| slot.kind == BaseKind::Si4),
            stamp: slot.stamp,
            expected_total: slot.total(),
            kind,
            changes,
        };
        self.start_job(
            target.slot,
            ListJobKind::SpellApply,
            move |request, control| {
                Effect::Spelling(SpellingEffect::ApplyNames {
                    request,
                    job: Box::new(job),
                    control,
                })
            },
            effects,
        );
        self.spelling.write = self.flow.list_job.as_ref().map(|j| j.request);
        self.spelling.write_kind = self.spelling.write.map(|_| kind);
        if self.spelling.write.is_some() {
            self.reads_pause(target.slot);
        }
    }

    /// Puts the database a write read again in the slot, and does what
    /// follows a change of the names or the ratings (rule 58).
    ///
    /// Returns whether the tags of the game on screen kept the old names
    /// because the game could not be read again.
    fn spelling_reloaded(&mut self, re: Reread, effects: &mut Vec<Effect>) -> bool {
        let Reread {
            slot: slot_id,
            source,
            stamp,
            in_memory,
            changed,
            names,
        } = re;
        if in_memory && changed {
            self.spelling.version = self.spelling.version.wrapping_add(1);
        }
        let Some(slot) = self.bases.slot_mut(slot_id) else {
            return false;
        };
        match source {
            Some(source) => {
                let total = GameSource::len(&*source.read());
                slot.source = source;
                slot.stamp = stamp;
                if total != slot.filter.len() {
                    slot.filter = mychess_db::Filter::new(total);
                    slot.list.select_only(None);
                    slot.clear_sort();
                }
            }
            None => slot.needs_reload |= !in_memory && changed,
        }
        if changed {
            slot.data_changed();
        }
        let name_sort = |c: ListColumn| {
            matches!(
                c,
                ListColumn::White
                    | ListColumn::Black
                    | ListColumn::Event
                    | ListColumn::Site
                    | ListColumn::Round
                    | ListColumn::WhiteElo
                    | ListColumn::BlackElo
            )
        };
        if changed && slot.list.sort.is_some_and(|(c, _)| name_sort(c)) {
            slot.clear_sort();
        }
        slot.clamp_first();
        let reload_game = if changed && names {
            slot.current_game
        } else {
            None
        };
        // The tags of the game on screen are the old names: read them again,
        // unless the board belongs to something else (a training, a game in
        // play, the trial mode) or the user is editing it (rule 58, D3472).
        let Some(number) = reload_game.filter(|_| slot_id == self.bases.current()) else {
            return false;
        };
        if !self.names_reload_allowed() {
            return true;
        }
        self.request_base_game(slot_id, number, None, effects);
        if let Some(pending) = self.flow.base_game.as_mut() {
            pending.names_refresh = true;
        }
        false
    }

    /// Whether the game on screen may be replaced by the same game read
    /// again (its names changed). `request_base_game` would end a training,
    /// and the answer would ask to discard an edited game.
    pub(super) fn names_reload_allowed(&self) -> bool {
        !self.is_dirty()
            && !self.edits_blocked()
            && !self.session.in_trial()
            && !self.flow.tactics.machine.is_live()
            && !self.flow.opening.machine.is_live()
            && !self.flow.calvar.machine.is_live()
            && !self.modal_open()
    }

    fn spell_applied(
        &mut self,
        request: RequestId,
        result: Result<NamesOutcome, crate::bases::BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(job) = self.finish_job(request) else {
            if self.spelling.write == Some(request) {
                self.spelling.write = None;
                self.spelling.write_kind = None;
            }
            // Another write may be in flight: it unblocks the reads (D3506).
            if self.spelling.write.is_none() {
                self.reads_unblock();
            }
            return;
        };
        let kind = self.spelling.write_kind.take().unwrap_or(NameKind::Player);
        self.spelling.write = None;
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                self.reads_resume(job.slot, false, effects);
                return self.job_failed(error);
            }
        };
        let report = outcome.report;
        let game_stale = self.spelling_reloaded(
            Reread {
                slot: job.slot,
                source: outcome.source,
                stamp: outcome.stamp,
                in_memory: outcome.in_memory,
                changed: report.games_changed > 0,
                names: true,
            },
            effects,
        );
        let notice = SpellingNotice::Applied {
            kind,
            names: report.mapped,
            times: report.games_changed,
            skipped_by_date: report.skipped_by_date,
            not_found: report.names_not_found,
            interrupted: report.interrupted,
        };
        if let Some(dialog) = self.spelling.check.as_mut() {
            dialog.result = Some(notice.clone());
            dialog.rows.clear();
            dialog.rebuild_rows();
            dialog.scanned = true;
        }
        self.spelling_notice(notice);
        if game_stale && report.games_changed > 0 {
            self.spelling_notice(SpellingNotice::GameNotRefreshed);
        }
        self.reads_resume(job.slot, false, effects);
        if let Some(error) = outcome.error
            && error != crate::bases::BaseError::Cancelled
        {
            self.job_failed(error);
        }
    }

    // ---- Add Elo ----

    fn elo_open(&mut self) {
        let Some(set) = self.spelling.set.clone() else {
            self.spelling_notice(SpellingNotice::NotLoaded);
            return;
        };
        if !set
            .checker(mychess_db::si4::NameKind::Player)
            .has_elo_data()
        {
            self.spelling_notice(SpellingNotice::NoEloData);
            return;
        }
        // Same as for the name checker (D3495, D3507).
        if self
            .spelling
            .add_elo
            .as_ref()
            .is_some_and(|d| d.writing.is_some())
        {
            return;
        }
        if self.spelling.write_deferred == Some(DeferredWrite::Elo) {
            self.spelling.write_deferred = None;
        }
        if let Some((run, _)) = self.spelling.add_elo.take().and_then(|old| old.planning) {
            run.control.cancel();
        }
        let prefs = self.prefs.player_tools.add_elo;
        self.spelling.add_elo = Some(AddEloDialog {
            slot: self.bases.current(),
            overwrite: prefs.overwrite,
            only_filter: prefs.only_filter,
            planned: None,
            planning: None,
            writing: None,
            result: None,
        });
    }

    fn elo_plan(&mut self, effects: &mut Vec<Effect>) {
        let Some(set) = self.spelling.set.clone() else {
            self.spelling_notice(SpellingNotice::NotLoaded);
            return;
        };
        if self
            .spelling
            .add_elo
            .as_ref()
            .is_some_and(|d| self.slot_rewriting(d.slot))
        {
            return;
        }
        if let Some(dialog) = self.spelling.add_elo.as_ref()
            && self.bases.slot(dialog.slot).is_some()
            && !self.slot_writable(dialog.slot)
        {
            self.spelling_notice(SpellingNotice::ReadOnly);
            return;
        }
        let request = self.new_request();
        let Some(dialog) = self.spelling.add_elo.as_mut() else {
            return;
        };
        if dialog.planning.is_some() || dialog.writing.is_some() {
            return;
        }
        let Some(slot) = self.bases.slot(dialog.slot) else {
            self.spelling_notice(SpellingNotice::NoBase);
            return;
        };
        dialog.planned = None;
        dialog.result = None;
        let control = OpenControl::new();
        let target = Target {
            slot: dialog.slot,
            source: slot.source.clone(),
            len: slot.total(),
            version: self.spelling.version,
        };
        dialog.planning = Some((
            Running {
                request,
                control: control.clone(),
            },
            target,
        ));
        let job = PlanJob {
            source: slot.source.clone(),
            set,
            filter: dialog.only_filter.then(|| Box::new(slot.filter.clone())),
            overwrite: dialog.overwrite,
            only_filter: dialog.only_filter,
        };
        effects.push(Effect::Spelling(SpellingEffect::PlanElo {
            request,
            job: Box::new(job),
            control,
        }));
    }

    fn elo_planned(&mut self, request: RequestId, result: Result<PlannedElo, SpellingNotice>) {
        let current = self
            .spelling
            .add_elo
            .as_ref()
            .and_then(|d| d.planning.as_ref())
            .map(|(run, _)| run.request);
        if current != Some(request) {
            return;
        }
        let Some(dialog) = self.spelling.add_elo.as_mut() else {
            return;
        };
        let Some((_, target)) = dialog.planning.take() else {
            return;
        };
        let still_there = self
            .bases
            .slot(target.slot)
            .is_some_and(|s| s.source == target.source)
            && self.spelling.version == target.version;
        if !still_there {
            return;
        }
        let notice = match result {
            Ok(planned) if planned.report.interrupted => return,
            Ok(planned) if planned.report.ratings == 0 => SpellingNotice::EloNothing,
            Ok(planned) => {
                dialog.planned = Some((target, planned));
                return;
            }
            Err(notice) => notice,
        };
        dialog.result = Some(notice.clone());
        self.spelling_notice(notice);
    }

    fn elo_write(&mut self, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.spelling.add_elo.as_ref() else {
            return;
        };
        let Some((target, planned)) = dialog.planned.clone() else {
            return;
        };
        if dialog.writing.is_some() {
            return;
        }
        if let Some(notice) = self.spell_refusal(&target) {
            self.spelling_notice(notice);
            return;
        }
        if self.list_busy() {
            return;
        }
        if self.defer_for_reads(target.slot, DeferredWrite::Elo) {
            return;
        }
        let Some(slot) = self.bases.slot(target.slot) else {
            return;
        };
        if slot.total() != target.len {
            self.spelling_notice(SpellingNotice::Stale);
            return;
        }
        let job = crate::spelling_ui::EloJob {
            source: slot.source.clone(),
            path: slot.path.clone().filter(|_| slot.kind == BaseKind::Si4),
            stamp: slot.stamp,
            expected_total: slot.total(),
            plan: planned.plan,
        };
        self.start_job(
            target.slot,
            ListJobKind::EloAdd,
            move |request, control| {
                Effect::Spelling(SpellingEffect::ApplyElo {
                    request,
                    job: Box::new(job),
                    control,
                })
            },
            effects,
        );
        let request = self.flow.list_job.as_ref().map(|j| j.request);
        self.spelling.write = request;
        if request.is_some() {
            self.reads_pause(target.slot);
        }
        if let Some(dialog) = self.spelling.add_elo.as_mut() {
            dialog.writing = request;
        }
    }

    fn elo_applied(
        &mut self,
        request: RequestId,
        result: Result<EloOutcome, crate::bases::BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(job) = self.finish_job(request) else {
            if self.spelling.write == Some(request) {
                self.spelling.write = None;
            }
            if let Some(dialog) = self.spelling.add_elo.as_mut() {
                dialog.writing = None;
            }
            if self.spelling.write.is_none() {
                self.reads_unblock();
            }
            return;
        };
        self.spelling.write = None;
        if let Some(dialog) = self.spelling.add_elo.as_mut() {
            dialog.writing = None;
        }
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                self.reads_resume(job.slot, true, effects);
                return self.job_failed(error);
            }
        };
        let report = outcome.report;
        self.spelling_reloaded(
            Reread {
                slot: job.slot,
                source: outcome.source,
                stamp: outcome.stamp,
                in_memory: outcome.in_memory,
                changed: report.games > 0,
                names: false,
            },
            effects,
        );
        let notice = if report.games == 0 && report.ratings == 0 {
            SpellingNotice::EloNothing
        } else {
            SpellingNotice::EloAdded {
                ratings: report.ratings,
                games: report.games,
            }
        };
        if let Some(dialog) = self.spelling.add_elo.as_mut() {
            dialog.planned = None;
            dialog.result = Some(notice.clone());
        }
        self.spelling_notice(notice);
        self.reads_resume(job.slot, true, effects);
        if let Some(error) = outcome.error
            && error != crate::bases::BaseError::Cancelled
        {
            self.job_failed(error);
        }
    }
}

/// Whether `message` is something the user does (as opposed to the answer of
/// a computation).
fn spelling_is_user_command(message: &SpellingMessage) -> bool {
    !matches!(
        message,
        SpellingMessage::Loaded { .. }
            | SpellingMessage::PhotosLoaded { .. }
            | SpellingMessage::StatsBuilt { .. }
            | SpellingMessage::Scanned { .. }
            | SpellingMessage::Applied { .. }
            | SpellingMessage::Planned { .. }
            | SpellingMessage::EloApplied { .. }
    )
}

#[cfg(test)]
#[path = "spelling_quit_tests.rs"]
mod quit_tests;
