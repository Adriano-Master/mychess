//! The maintenance tools in the state (spec `docs/specs/m7-1-manutencao.md`,
//! slice M7-1b): one window, one operation at a time (a job of the game
//! list), confirmations with the count of what will be affected, the checks
//! of read-only, PGN and Clipbase, and the bookkeeping after a write (the
//! filter, the selection and the open game follow a sort; the caches and the
//! windows that follow the database are told that it changed).
//!
//! `update` only changes the state and returns effects. The work is in
//! `maintenance/work.rs`; a job answers with [`MaintenanceMessage::Done`].

use mychess_db::Filter;
use mychess_db::GameSource;
use mychess_db::si4::{FlagOp, NameEdit, NameEditTarget, NameKind, SortKey, StripWhat, TagOp};

use super::AppState;
use crate::bases::{BaseError, BaseKind, BasesNotice, ListJobKind, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::{AppNotice, DiscardPrompt, Pending, Replacement};
use crate::maintenance::CleanerJob;
use crate::maintenance::{
    Checker, ConfirmKind, ConfirmView, FlagAction, FlagScope, GameSet, Gate, HeaderEdit, JobWhat,
    MAX_SORT_CRITERIA, MaintFlag, MaintJob, MaintJobKind, MaintNotice, MaintOp, MaintOutcome,
    MaintReport, MaintRequest, MaintenanceEffect, MaintenanceMessage, MaintenancePrefs,
    NameJobGames, NameKindEdit, NameRequest, NameRequestKind, NameScopeChoice, Prepared,
    PreviewDone, PreviewJob, PreviewResult, PreviewWork, Remember, RunningJob, ScopeChoice, Stage,
    Staged, StatsEntry, StatsRun, StripChoice, TagRequest, TwinSide, TwinState, WeakCriteria,
};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::spelling_ui::Shared;

/// What a request needs of the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// Any database (a read).
    Any,
    /// An `.si4` that can be written, or the Clipbase.
    Editable,
    /// An `.si4` that can be written.
    Si4,
}

fn need_of(request: &MaintRequest) -> Need {
    match request {
        MaintRequest::Flags { .. }
        | MaintRequest::FindTwins
        | MaintRequest::Tags(TagRequest::List | TagRequest::Filter(_)) => Need::Editable,
        MaintRequest::DropDeletedFromFilter
        | MaintRequest::Check { .. }
        | MaintRequest::Repair { .. } => Need::Any,
        MaintRequest::Header(_)
        | MaintRequest::Compact { .. }
        | MaintRequest::Sort { .. }
        | MaintRequest::Tags(_)
        | MaintRequest::Strip { .. }
        | MaintRequest::Names(_)
        | MaintRequest::Cleaner => Need::Si4,
    }
}

/// The notice for a tag name or value the database would refuse.
fn tag_text_ok(name: &str, value: Option<&str>) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && value.is_none_or(|v| v.len() <= 240 && !v.contains(['\r', '\n', '\0']))
}

fn db_kind(kind: &NameKindEdit) -> NameKind {
    match kind {
        NameKindEdit::Player => NameKind::Player,
        NameKindEdit::Event => NameKind::Event,
        NameKindEdit::Site => NameKind::Site,
        NameKindEdit::Round => NameKind::Round,
    }
}

fn name_edit_of(request: &NameRequest) -> NameEdit {
    let target = match &request.kind {
        NameRequestKind::Name { kind, old, new } => NameEditTarget::Name {
            kind: db_kind(kind),
            old: old.clone(),
            new: new.trim().to_owned(),
        },
        NameRequestKind::Rating {
            player,
            elo,
            rating_type,
        } => NameEditTarget::Rating {
            player: player.clone(),
            elo: *elo,
            rating_type: *rating_type,
        },
        NameRequestKind::Date { old, new } => NameEditTarget::Date {
            old: old.clone(),
            new: new.clone(),
        },
        NameRequestKind::EventDate { old, new } => NameEditTarget::EventDate {
            old: old.clone(),
            new: new.clone(),
        },
    };
    NameEdit {
        target,
        pattern: request.pattern,
    }
}

impl AppState {
    fn maint_notice(&mut self, notice: MaintNotice) {
        self.notice = Some(AppNotice::Maintenance(notice));
    }

    /// The pairs of the last search for duplicates, while the database is
    /// the one they were found in (they are dropped by any write that can
    /// change them, rule 22).
    pub(crate) fn maint_twins(&self) -> Option<&TwinState> {
        let twins = self.maintenance.twins.as_ref()?;
        let slot = self.bases.slot(twins.slot)?;
        (slot.source == twins.source && slot.data_rev() == twins.data_rev).then_some(twins)
    }

    /// Whether a maintenance job is rewriting the database of `slot`: its
    /// files must not be read meanwhile (the windows that follow the
    /// database wait).
    pub(crate) fn maintenance_rewriting(&self, slot: SlotId) -> bool {
        self.maintenance
            .running
            .as_ref()
            .is_some_and(|r| r.what.writes() && r.slot == slot)
    }

    // ---- the entry points ----

    /// Handles a [`MaintenanceMessage`].
    pub(crate) fn maintenance_message(
        &mut self,
        message: MaintenanceMessage,
        effects: &mut Vec<Effect>,
    ) {
        self.maintenance_heal_quit(&message, effects);
        self.maintenance_message_inner(message, effects);
        // A quit waiting for the job goes on when the job is done (D589).
        if self.flow.quit_waiting_job && self.flow.list_job.is_none() {
            self.flow.quit_waiting_job = false;
            self.finish_quit(effects);
        }
    }

    /// Stops what runs (the program is ending). The statistics are dropped
    /// and start again if the quit is given up.
    pub(crate) fn maintenance_cancel(&mut self) {
        self.maintenance.cancel();
        self.maintenance.stats_run = None;
    }

    /// The quit did not end the program: what `maintenance_cancel` stopped
    /// starts again (the end of the update brings the statistics back).
    pub(crate) fn maintenance_quit_undone(&mut self) {
        self.maintenance.quitting = false;
    }

    /// The first user command after a quit that did not end the program and
    /// told nothing to this flow: the quit is undone here (like the safety
    /// net of the spelling flow, D3505).
    fn maintenance_heal_quit(&mut self, message: &MaintenanceMessage, effects: &mut Vec<Effect>) {
        let user = !matches!(
            message,
            MaintenanceMessage::Done { .. }
                | MaintenanceMessage::StatsDone { .. }
                | MaintenanceMessage::Previewed { .. }
                | MaintenanceMessage::Diffed { .. }
                | MaintenanceMessage::SharePlanned { .. }
        );
        if self.maintenance.quitting && user && self.quit_not_going_on() {
            self.spelling_quit_undone(effects);
            self.maintenance_quit_undone();
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the messages
    fn maintenance_message_inner(
        &mut self,
        message: MaintenanceMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            MaintenanceMessage::Open => self.maintenance.open = true,
            MaintenanceMessage::Close => {
                self.maintenance.open = false;
                self.maintenance.checker = None;
                if let Some(run) = self.maintenance.stats_run.take() {
                    run.control.cancel();
                }
            }
            MaintenanceMessage::Refresh => {
                self.maintenance.stats = None;
                if let Some(run) = self.maintenance.stats_run.take() {
                    run.control.cancel();
                }
            }
            MaintenanceMessage::Start(request) => self.maint_start(request, effects),
            MaintenanceMessage::Confirm(yes) => self.maint_confirm(yes, effects),
            MaintenanceMessage::SetBackup(on) => {
                if let Some(confirm) = self
                    .maintenance
                    .staged
                    .as_mut()
                    .and_then(|s| s.confirm.as_mut())
                    .filter(|c| c.backup_offered)
                {
                    confirm.backup = on;
                }
            }
            MaintenanceMessage::AutoLoadText(text) => match text.trim().parse::<u32>() {
                Ok(game) if game < mychess_db::si4::MAX_GAMES => {
                    self.maint_start(MaintRequest::Header(HeaderEdit::AutoLoad(game)), effects);
                }
                _ => self.maint_notice(MaintNotice::InvalidValue),
            },
            MaintenanceMessage::SetPrefs(prefs) => {
                let prefs = prefs.sanitized();
                if prefs != *self.prefs.maintenance {
                    self.prefs_message(
                        PrefsMessage::Set(PrefChange::Maintenance(Box::new(prefs))),
                        effects,
                    );
                }
            }
            MaintenanceMessage::Stop => {
                // A preview that counts is stopped for real (the lock of the
                // database goes with it).
                if let Some(control) = self.maintenance.preview.take() {
                    control.cancel();
                }
                if let Some(run) = &self.maintenance.running
                    && let Some(job) = self
                        .flow
                        .list_job
                        .as_ref()
                        .filter(|j| j.request == run.request)
                {
                    job.control.cancel();
                }
                // A question that waits for the user is dropped too.
                if self
                    .maintenance
                    .staged
                    .as_ref()
                    .is_some_and(|s| s.stage != Stage::Discarding)
                {
                    self.maintenance.staged = None;
                }
            }
            MaintenanceMessage::OpenChecker => self.checker_open(effects),
            MaintenanceMessage::CloseChecker => self.maintenance.checker = None,
            MaintenanceMessage::TwinStep(direction) => self.checker_step(direction, effects),
            MaintenanceMessage::TwinToggle(side) => self.checker_toggle(&[side], effects),
            MaintenanceMessage::TwinToggleBoth => {
                self.checker_toggle(&[TwinSide::Left, TwinSide::Right], effects);
            }
            MaintenanceMessage::TwinUndeleteAll => self.checker_undelete_all(),
            MaintenanceMessage::TwinShare => self.checker_share(effects),
            MaintenanceMessage::Done { request, result } => {
                self.maint_done(request, *result, effects);
            }
            MaintenanceMessage::StatsDone { request, stats } => self.stats_done(request, stats),
            MaintenanceMessage::Previewed { request, result } => {
                self.maint_previewed(request, *result, effects);
            }
            MaintenanceMessage::Diffed { request, diff } => {
                if let Some(checker) = self.maintenance.checker.as_mut()
                    && checker.diff_request == Some(request)
                {
                    checker.diff_request = None;
                    checker.diff = diff;
                }
            }
            MaintenanceMessage::SharePlanned { request, tags } => {
                self.share_planned(request, tags);
            }
        }
    }

    // ---- the statistics ----

    /// Counts the database of the current slot when the window is open and
    /// the numbers on show are not those of its games (called at the end of
    /// every `update`).
    pub(crate) fn refresh_maintenance(&mut self, effects: &mut Vec<Effect>) {
        if !self.maintenance.open || self.maintenance.quitting {
            return;
        }
        let id = self.bases.current();
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        if slot.kind != BaseKind::Si4 || slot.needs_reload {
            self.maintenance.stats = None;
            return;
        }
        // A write under way: its end changes the database and the counting
        // starts then.
        if self.flow.list_job.is_some() || self.flow.annotate_batch.is_some() {
            return;
        }
        let (source, data_rev) = (slot.source.clone(), slot.data_rev());
        let fresh = self
            .maintenance
            .stats
            .as_ref()
            .is_some_and(|s| s.slot == id && s.source == source && s.data_rev == data_rev);
        let running = self
            .maintenance
            .stats_run
            .as_ref()
            .is_some_and(|r| r.slot == id && r.source == source && r.data_rev == data_rev);
        if fresh || running {
            return;
        }
        if let Some(old) = self.maintenance.stats_run.take() {
            old.control.cancel();
        }
        let request = self.new_request();
        let control = crate::bases::OpenControl::new();
        self.maintenance.stats_run = Some(StatsRun {
            request,
            slot: id,
            source: source.clone(),
            data_rev,
            control: control.clone(),
        });
        effects.push(Effect::Maintenance(MaintenanceEffect::Stats {
            request,
            source,
            control,
        }));
    }

    fn stats_done(&mut self, request: RequestId, stats: Option<Shared<mychess_db::DatabaseStats>>) {
        let Some(run) = self.maintenance.stats_run.take_if(|r| r.request == request) else {
            return;
        };
        let Some(stats) = stats else {
            return;
        };
        let still = self
            .bases
            .slot(run.slot)
            .is_some_and(|s| s.source == run.source && s.data_rev() == run.data_rev);
        if still {
            self.maintenance.stats = Some(StatsEntry {
                slot: run.slot,
                source: run.source,
                data_rev: run.data_rev,
                stats,
            });
        }
    }

    /// Games with the `D` flag, from the statistics on show.
    fn maint_deleted_count(&self, id: SlotId) -> u32 {
        let Some(slot) = self.bases.slot(id) else {
            return 0;
        };
        self.maintenance
            .stats
            .as_ref()
            .filter(|s| s.slot == id && s.source == slot.source && s.data_rev == slot.data_rev())
            .map_or(0, |s| {
                s.stats.flag_count(mychess_db::si4::GameFlags::DELETE)
            })
    }

    // ---- what a database allows ----

    fn maint_refusal(&self, id: SlotId, need: Need) -> Option<MaintNotice> {
        let Some(slot) = self.bases.slot(id) else {
            return Some(MaintNotice::Failed(BaseError::Internal));
        };
        if slot.needs_reload {
            return Some(MaintNotice::Failed(BaseError::Stale));
        }
        match (need, slot.kind) {
            (Need::Any, _) | (Need::Editable, BaseKind::Clipbase) => None,
            (_, BaseKind::Pgn) => Some(MaintNotice::Pgn),
            (Need::Si4, BaseKind::Clipbase) => Some(MaintNotice::ClipbaseUnsupported),
            (_, BaseKind::Si4) if slot.read_only => Some(MaintNotice::ReadOnly),
            _ => None,
        }
    }

    // ---- asking for an operation ----

    fn maint_start(&mut self, request: MaintRequest, effects: &mut Vec<Effect>) {
        if let MaintRequest::Repair {
            path,
            drop_unreadable,
        } = request
        {
            self.maint_start_repair(path, drop_unreadable);
            return;
        }
        let id = self.bases.current();
        if let Some(notice) = self.maint_refusal(id, need_of(&request)) {
            self.maint_notice(notice);
            return;
        }
        if self.discard_prompt.is_some() || self.list_busy() {
            return;
        }
        // A new request replaces a question nobody answered.
        if let Some(control) = self.maintenance.preview.take() {
            control.cancel();
        }
        self.maintenance.staged = None;
        let prepared = match self.maint_prepare(id, &request) {
            Ok(prepared) => prepared,
            Err(notice) => {
                self.maint_notice(notice);
                return;
            }
        };
        self.maint_stage(id, prepared, effects);
    }

    fn scope_set(&self, id: SlotId, scope: ScopeChoice) -> (GameSet, Option<Box<Filter>>, u32) {
        let Some(slot) = self.bases.slot(id) else {
            return (GameSet::All, None, 0);
        };
        match scope {
            ScopeChoice::All => (GameSet::All, None, slot.total()),
            ScopeChoice::Filter => (
                GameSet::Filter(Box::new(slot.filter.clone())),
                Some(Box::new(slot.filter.clone())),
                slot.filtered(),
            ),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the requests
    fn maint_prepare(&self, id: SlotId, request: &MaintRequest) -> Result<Prepared, MaintNotice> {
        let slot = self
            .bases
            .slot(id)
            .ok_or(MaintNotice::Failed(BaseError::Internal))?;
        let total = slot.total();
        let prefs = &*self.prefs.maintenance;
        let base = |kind: MaintJobKind, what: JobWhat, op: MaintOp| Prepared {
            kind,
            what,
            op,
            gate: Gate::Open,
            rewrites: false,
            count: 0,
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        match request {
            MaintRequest::Flags {
                scope,
                flag,
                action,
            } => {
                let bits = flag.bits().ok_or(MaintNotice::InvalidValue)?;
                let games = match scope {
                    FlagScope::Current => {
                        let number = slot
                            .current_game
                            .filter(|n| (1..=total).contains(n))
                            .ok_or(MaintNotice::NoCurrentGame)?;
                        GameSet::Numbers(vec![number])
                    }
                    FlagScope::Filter => GameSet::Filter(Box::new(slot.filter.clone())),
                    FlagScope::All => GameSet::All,
                };
                Ok(base(
                    MaintJobKind::Flags,
                    JobWhat::Flags { flag: *flag },
                    MaintOp::Flags {
                        games,
                        flag: bits,
                        action: flag_op(*action),
                    },
                ))
            }
            MaintRequest::Header(edit) => {
                match edit {
                    HeaderEdit::BaseType(value) => {
                        let current = slot
                            .source
                            .try_read()
                            .map(|db| GameSource::header(&*db).base_type());
                        // The system types are not chosen by hand (1 to 3).
                        if *value > 31 || ((1..=3).contains(value) && current != Some(*value)) {
                            return Err(MaintNotice::InvalidValue);
                        }
                    }
                    HeaderEdit::CustomFlag { number, .. } if !(1..=6).contains(number) => {
                        return Err(MaintNotice::InvalidValue);
                    }
                    HeaderEdit::AutoLoad(game) if *game >= mychess_db::si4::MAX_GAMES => {
                        return Err(MaintNotice::InvalidValue);
                    }
                    _ => {}
                }
                Ok(base(
                    MaintJobKind::Flags,
                    JobWhat::Header,
                    MaintOp::Header(edit.clone()),
                ))
            }
            MaintRequest::DropDeletedFromFilter => Ok(base(
                MaintJobKind::Flags,
                JobWhat::DropDeleted,
                MaintOp::DropDeleted {
                    filter: Box::new(slot.filter.clone()),
                },
            )),
            MaintRequest::Check { scope } => {
                let (_, filter, _) = self.scope_set(id, *scope);
                Ok(base(
                    MaintJobKind::Check,
                    JobWhat::Check,
                    MaintOp::Check { filter },
                ))
            }
            MaintRequest::Compact { names, games } => {
                if !names && !games {
                    return Err(MaintNotice::NothingToDo);
                }
                let mut p = base(
                    MaintJobKind::Compact,
                    JobWhat::Compact,
                    MaintOp::Compact {
                        names: *names,
                        games: *games,
                    },
                );
                p.gate = Gate::Preview(PreviewWork::Compact, ConfirmKind::Compact);
                p.rewrites = *games;
                p.backup_offered = true;
                Ok(p)
            }
            MaintRequest::Sort {
                keys,
                reverse,
                compact_after,
            } => {
                if keys.is_empty() || keys.len() > MAX_SORT_CRITERIA {
                    return Err(MaintNotice::BadSort);
                }
                let sort_keys: Vec<SortKey> = keys.iter().map(|k| k.key()).collect();
                let mut p = base(
                    MaintJobKind::Sort,
                    JobWhat::Sort,
                    MaintOp::Sort {
                        keys: sort_keys,
                        reverse: *reverse,
                        compact_after: *compact_after,
                    },
                );
                p.gate = Gate::Confirm(ConfirmKind::Sort);
                p.count = total;
                p.rewrites = *compact_after;
                p.backup_offered = true;
                p.remember = Remember::Sort {
                    keys: keys.clone(),
                    reverse: *reverse,
                    compact_after: *compact_after,
                };
                Ok(p)
            }
            MaintRequest::FindTwins => {
                let twins = &prefs.twins;
                let mut weak = Vec::new();
                if twins.weak_without_moves() {
                    weak.push(WeakCriteria::NoMoves);
                }
                if twins.weak_few_fields() {
                    weak.push(WeakCriteria::FewFields);
                }
                let filter = twins.only_filter.then(|| Box::new(slot.filter.clone()));
                let mut p = if twins.undelete_first {
                    let mut p = base(
                        MaintJobKind::Twins,
                        JobWhat::UndeleteFirst,
                        MaintOp::UndeleteAll,
                    );
                    p.gate = Gate::Confirm(ConfirmKind::UndeleteFirst);
                    p.count = self.maint_deleted_count(id);
                    p.backup_offered = true;
                    p
                } else {
                    let mut p = base(
                        MaintJobKind::Twins,
                        JobWhat::Twins,
                        MaintOp::Twins {
                            criteria: twins.criteria(),
                            filter,
                        },
                    );
                    if !weak.is_empty() {
                        p.gate = Gate::Confirm(ConfirmKind::WeakTwinCriteria);
                    }
                    p
                };
                p.weak = weak;
                Ok(p)
            }
            MaintRequest::Repair { .. } => Err(MaintNotice::Failed(BaseError::Internal)),
            MaintRequest::Tags(tags) => self.prepare_tags(id, tags),
            MaintRequest::Strip { what, scope } => {
                let (games, _, count) = self.scope_set(id, *scope);
                let what = match what {
                    StripChoice::Comments => StripWhat::Comments,
                    StripChoice::Variations => StripWhat::Variations,
                };
                let mut p = base(
                    MaintJobKind::Strip,
                    JobWhat::Strip,
                    MaintOp::Strip {
                        what,
                        games: games.clone(),
                    },
                );
                p.gate = Gate::Preview(
                    PreviewWork::Strip { what, games, total },
                    ConfirmKind::Strip,
                );
                p.count = count;
                p.rewrites = true;
                p.backup_offered = true;
                p.remember = Remember::StripScope(*scope);
                Ok(p)
            }
            MaintRequest::Names(names) => {
                if let NameRequestKind::Name { new, .. } = &names.kind
                    && new.trim().is_empty()
                {
                    return Err(MaintNotice::InvalidValue);
                }
                let (games, count) = match names.scope {
                    NameScopeChoice::All => (NameJobGames::All, total),
                    NameScopeChoice::Filter => (
                        NameJobGames::Set(GameSet::Filter(Box::new(slot.filter.clone()))),
                        slot.filtered(),
                    ),
                    NameScopeChoice::Tournament => {
                        let game = slot
                            .current_game
                            .filter(|n| (1..=total).contains(n))
                            .ok_or(MaintNotice::NoCurrentGame)?;
                        (NameJobGames::Tournament(game), 0)
                    }
                };
                let mut p = base(
                    MaintJobKind::Names,
                    JobWhat::Names,
                    MaintOp::Names {
                        edit: name_edit_of(names),
                        games,
                    },
                );
                p.gate = Gate::Confirm(ConfirmKind::Names);
                p.count = count;
                p.rewrites = true;
                p.backup_offered = true;
                p.remember = Remember::NameScope(names.scope);
                Ok(p)
            }
            MaintRequest::Cleaner => {
                let steps = prefs.cleaner;
                let any = steps.players
                    || steps.events
                    || steps.sites
                    || steps.rounds
                    || steps.eco
                    || steps.elo
                    || steps.twins
                    || steps.compact_names
                    || steps.compact_games
                    || steps.tree;
                if !any {
                    return Err(MaintNotice::NothingToDo);
                }
                let job = CleanerJob {
                    steps,
                    twins: prefs.twins.criteria(),
                    undelete_first: prefs.twins.undelete_first,
                    twins_filter: prefs
                        .twins
                        .only_filter
                        .then(|| Box::new(slot.filter.clone())),
                    spelling: self.spelling.set.clone(),
                    eco_book: self.eco_book.clone().filter(|book| !book.is_empty()),
                };
                let mut p = base(
                    MaintJobKind::Cleaner,
                    JobWhat::Cleaner,
                    MaintOp::Cleaner(Box::new(job)),
                );
                p.gate = Gate::Confirm(ConfirmKind::Cleaner);
                p.rewrites = true;
                p.backup_offered = true;
                Ok(p)
            }
        }
    }

    fn prepare_tags(&self, id: SlotId, request: &TagRequest) -> Result<Prepared, MaintNotice> {
        let prefs = &*self.prefs.maintenance;
        let scope = prefs.tag_scope;
        let (games, filter, count) = self.scope_set(id, scope);
        let base = |what: JobWhat, op: MaintOp| Prepared {
            kind: MaintJobKind::Tags,
            what,
            op,
            gate: Gate::Open,
            rewrites: false,
            count: 0,
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        match request {
            TagRequest::List => Ok(base(
                JobWhat::TagList { scope },
                MaintOp::TagList { filter },
            )),
            TagRequest::Filter(tag) => {
                if !tag_text_ok(tag, None) {
                    return Err(MaintNotice::InvalidValue);
                }
                Ok(base(
                    JobWhat::TagFilter,
                    MaintOp::TagFilter {
                        tag: tag.clone(),
                        filter,
                    },
                ))
            }
            TagRequest::Add { name, value } => {
                if !tag_text_ok(name, Some(value)) {
                    return Err(MaintNotice::InvalidValue);
                }
                let mut p = base(
                    JobWhat::TagEdit,
                    MaintOp::TagEdit {
                        op: TagOp::Add {
                            name: name.clone(),
                            value: value.clone(),
                        },
                        games,
                    },
                );
                p.gate = Gate::Confirm(ConfirmKind::TagAdd);
                p.count = count;
                p.rewrites = true;
                p.backup_offered = true;
                Ok(p)
            }
            TagRequest::Strip(name) => {
                if !tag_text_ok(name, None) {
                    return Err(MaintNotice::InvalidValue);
                }
                // The listing knows how many games have the tag, when it was
                // made for this scope.
                let listed = self
                    .maintenance
                    .tags
                    .as_ref()
                    .filter(|view| view.scope == scope)
                    .and_then(|view| view.listing.tags.iter().find(|t| &t.name == name))
                    .map(|t| t.games);
                let mut p = base(
                    JobWhat::TagEdit,
                    MaintOp::TagEdit {
                        op: TagOp::Strip { name: name.clone() },
                        games,
                    },
                );
                p.gate = Gate::Confirm(ConfirmKind::TagStrip);
                p.count = listed.unwrap_or(count);
                p.rewrites = true;
                p.backup_offered = true;
                Ok(p)
            }
        }
    }

    /// Starts the repair of a database that is not open.
    fn maint_start_repair(&mut self, path: std::path::PathBuf, drop_unreadable: bool) {
        let is_si4 = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("si4"));
        if !is_si4 {
            self.maint_notice(MaintNotice::RepairNotSi4);
            return;
        }
        let open: Vec<std::path::PathBuf> = self
            .bases
            .iter()
            .filter_map(|(_, slot)| slot.path.clone())
            .collect();
        if open.iter().any(|p| p == &path) {
            self.maint_notice(MaintNotice::RepairOpen);
            return;
        }
        if self.discard_prompt.is_some() || self.list_busy() {
            return;
        }
        let id = self.bases.current();
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let prepared = Prepared {
            kind: MaintJobKind::Repair,
            what: JobWhat::Repair,
            op: MaintOp::Repair {
                drop_unreadable,
                open,
            },
            gate: Gate::Confirm(ConfirmKind::Repair),
            rewrites: false,
            count: 0,
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        self.maintenance.staged = Some(Staged {
            slot: id,
            source: slot.source.clone(),
            confirm: Some(confirm_view(&prepared)),
            prepared,
            stage: Stage::Confirming,
            discarded: false,
        });
        self.maintenance.repair_path = Some(path);
    }

    fn maint_stage(&mut self, id: SlotId, prepared: Prepared, effects: &mut Vec<Effect>) {
        let Some(source) = self.bases.slot(id).map(|s| s.source.clone()) else {
            return;
        };
        let mut staged = Staged {
            slot: id,
            source: source.clone(),
            confirm: None,
            stage: Stage::Confirming,
            discarded: false,
            prepared,
        };
        match staged.prepared.gate.clone() {
            Gate::Open => self.maint_begin(staged, effects),
            Gate::Confirm(_) => {
                staged.confirm = Some(confirm_view(&staged.prepared));
                self.maintenance.staged = Some(staged);
            }
            Gate::Preview(work, _) => {
                let request = self.new_request();
                let control = crate::bases::OpenControl::new();
                staged.stage = Stage::Previewing(request);
                self.maintenance.staged = Some(staged);
                self.maintenance.preview = Some(control.clone());
                effects.push(Effect::Maintenance(MaintenanceEffect::Preview {
                    request,
                    source,
                    work: Box::new(PreviewJob(work)),
                    control,
                }));
            }
        }
    }

    fn maint_previewed(
        &mut self,
        request: RequestId,
        result: Option<PreviewDone>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(mut staged) = self
            .maintenance
            .staged
            .take_if(|s| s.stage == Stage::Previewing(request))
        else {
            return;
        };
        self.maintenance.preview = None;
        let current = self
            .bases
            .slot(staged.slot)
            .is_some_and(|s| s.source == staged.source);
        if !current {
            self.maint_notice(MaintNotice::Failed(BaseError::Stale));
            return;
        }
        let Some(PreviewDone(result)) = result else {
            // A quit that was given up stopped the count: nothing to tell.
            if !self.maintenance.quitting {
                self.maint_notice(MaintNotice::Failed(BaseError::Internal));
            }
            return;
        };
        let Gate::Preview(_, kind) = staged.prepared.gate.clone() else {
            return;
        };
        match result {
            PreviewResult::Strip(affected) => {
                if affected == 0 {
                    self.maint_notice(MaintNotice::NothingToDo);
                    return;
                }
                staged.prepared.count = affected;
            }
            PreviewResult::Compact(preview) => {
                let MaintOp::Compact { names, games } = staged.prepared.op else {
                    return;
                };
                let names_todo = names && preview.names.iter().any(|u| u.unused > 0);
                let games_todo = games && !preview.stats.is_compact();
                if !names_todo && !games_todo {
                    self.maint_notice(MaintNotice::NothingToDo);
                    return;
                }
                // What is not to be done is not asked for.
                staged.prepared.op = MaintOp::Compact {
                    names: names_todo,
                    games: games_todo,
                };
                staged.prepared.rewrites = games_todo;
                if !(games_todo && preview.stats.deleted_games > 0) {
                    // Nothing is deleted for good: no question.
                    self.maint_begin(staged, effects);
                    return;
                }
                staged.prepared.count = preview.stats.deleted_games;
            }
        }
        staged.prepared.gate = Gate::Confirm(kind);
        staged.confirm = Some(confirm_view(&staged.prepared));
        staged.stage = Stage::Confirming;
        self.maintenance.staged = Some(staged);
    }

    fn maint_confirm(&mut self, yes: bool, effects: &mut Vec<Effect>) {
        let Some(staged) = self
            .maintenance
            .staged
            .take_if(|s| s.stage == Stage::Confirming)
        else {
            return;
        };
        if !yes {
            self.maintenance.repair_path = None;
            return;
        }
        if let MaintOp::Repair { .. } = staged.prepared.op {
            self.maint_launch_repair(staged, effects);
            return;
        }
        self.maint_begin(staged, effects);
    }

    /// Asks the altered game of the editor first when the operation
    /// rewrites the games of its database (spec rule 3), then starts.
    fn maint_begin(&mut self, mut staged: Staged, effects: &mut Vec<Effect>) {
        if staged.prepared.rewrites
            && staged.slot == self.bases.current()
            && self.needs_confirmation()
        {
            staged.stage = Stage::Discarding;
            self.maintenance.staged = Some(staged);
            self.proceed_or_ask(Pending::Maintenance, effects);
            // Refused without a question (an annotation runs...): the
            // operation is not waiting for anything.
            let asked = matches!(
                self.discard_prompt,
                Some(DiscardPrompt {
                    pending: Pending::Maintenance
                })
            );
            if !asked {
                self.maintenance.staged = None;
            }
            return;
        }
        self.maint_launch(staged, effects);
    }

    /// An operation waits for the answer about the altered game of the
    /// editor (for tests of the flow).
    #[doc(hidden)]
    #[must_use]
    pub fn maintenance_waits_for_save(&self) -> bool {
        self.maintenance
            .staged
            .as_ref()
            .is_some_and(|s| s.stage == Stage::Discarding)
    }

    /// The user gave up "save the altered game?" (or the save failed): the
    /// operation that waited for it is dropped, the window is free again.
    pub(crate) fn maintenance_give_up(&mut self) {
        self.maintenance
            .staged
            .take_if(|s| s.stage == Stage::Discarding);
    }

    /// The answer to "save the altered game?" for the operation that waits.
    pub(crate) fn maintenance_confirmed(&mut self, effects: &mut Vec<Effect>) {
        let Some(mut staged) = self
            .maintenance
            .staged
            .take_if(|s| s.stage == Stage::Discarding)
        else {
            return;
        };
        // "Don't save" leaves the altered game on the editor: it goes when
        // the games change under it.
        staged.discarded = self.session.is_dirty();
        self.maint_launch(staged, effects);
    }

    fn maint_set_prefs(
        &mut self,
        change: impl FnOnce(&mut MaintenancePrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut next = (*self.prefs.maintenance).clone();
        change(&mut next);
        let next = next.sanitized();
        if next != *self.prefs.maintenance {
            self.prefs_message(
                PrefsMessage::Set(PrefChange::Maintenance(Box::new(next))),
                effects,
            );
        }
    }

    /// Stops what reads the database of `id` and is about to be rewritten:
    /// the counting of the statistics, the finders, the checker of names.
    fn maint_stop_reads(&mut self, id: SlotId) {
        if let Some(run) = self.maintenance.stats_run.take() {
            run.control.cancel();
        }
        self.reads_pause(id);
    }

    fn maint_launch(&mut self, staged: Staged, effects: &mut Vec<Effect>) {
        let Staged {
            slot: id,
            source,
            prepared,
            confirm,
            discarded,
            ..
        } = staged;
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        // The question was open for a while: the database may be another.
        if slot.source != source {
            self.maint_notice(MaintNotice::Failed(BaseError::Stale));
            return;
        }
        let need = match &prepared.op {
            MaintOp::Check { .. } | MaintOp::DropDeleted { .. } => Need::Any,
            MaintOp::Flags { .. }
            | MaintOp::UndeleteAll
            | MaintOp::MarkTwins { .. }
            | MaintOp::Twins { .. }
            | MaintOp::TagList { .. }
            | MaintOp::TagFilter { .. } => Need::Editable,
            _ => Need::Si4,
        };
        if let Some(notice) = self.maint_refusal(id, need) {
            self.maint_notice(notice);
            return;
        }
        if self.list_busy() {
            self.maint_notice(MaintNotice::Busy);
            return;
        }
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let path = slot.path.clone().filter(|_| slot.kind == BaseKind::Si4);
        let job = MaintJob {
            source: slot.source.clone(),
            path,
            stamp: slot.stamp,
            expected_total: slot.total(),
            op: prepared.op,
            backup: confirm.as_ref().is_some_and(|c| c.backup),
        };
        let (kind, what) = (prepared.kind, prepared.what);
        match prepared.remember {
            Remember::Nothing => {}
            Remember::Sort {
                keys,
                reverse,
                compact_after,
            } => self.maint_set_prefs(
                move |p| {
                    p.sort_keys = keys;
                    p.sort_reverse = reverse;
                    p.compact_after_rewrite = compact_after;
                },
                effects,
            ),
            Remember::StripScope(scope) => {
                self.maint_set_prefs(move |p| p.strip_scope = scope, effects);
            }
            Remember::NameScope(scope) => {
                self.maint_set_prefs(move |p| p.name_editor_scope = scope, effects);
            }
        }
        if what.writes() {
            self.maint_stop_reads(id);
        }
        self.start_job(
            id,
            ListJobKind::Flags,
            move |request, control| {
                Effect::Maintenance(MaintenanceEffect::Run {
                    request,
                    job: Box::new(job),
                    control,
                })
            },
            effects,
        );
        let Some(request) = self.flow.list_job.as_ref().map(|j| j.request) else {
            return;
        };
        self.maintenance.running = Some(RunningJob {
            request,
            kind,
            slot: id,
            detached: false,
            what,
            discarded,
        });
        self.maintenance.backups.clear();
    }

    fn maint_launch_repair(&mut self, staged: Staged, effects: &mut Vec<Effect>) {
        let Some(path) = self.maintenance.repair_path.take() else {
            return;
        };
        let Staged {
            slot: id,
            source,
            prepared,
            ..
        } = staged;
        if self.list_busy() {
            self.maint_notice(MaintNotice::Busy);
            return;
        }
        // The databases may have changed while the question was open: the
        // list of the open files is the one of now.
        let open: Vec<std::path::PathBuf> = self
            .bases
            .iter()
            .filter_map(|(_, slot)| slot.path.clone())
            .collect();
        let op = match prepared.op {
            MaintOp::Repair {
                drop_unreadable, ..
            } => {
                if open.iter().any(|p| p == &path) {
                    self.maint_notice(MaintNotice::RepairOpen);
                    return;
                }
                MaintOp::Repair {
                    drop_unreadable,
                    open,
                }
            }
            other => other,
        };
        let job = MaintJob {
            source,
            path: Some(path),
            stamp: None,
            expected_total: 0,
            op,
            backup: false,
        };
        self.start_job(
            id,
            ListJobKind::Flags,
            move |request, control| {
                Effect::Maintenance(MaintenanceEffect::Run {
                    request,
                    job: Box::new(job),
                    control,
                })
            },
            effects,
        );
        let Some(request) = self.flow.list_job.as_ref().map(|j| j.request) else {
            return;
        };
        // The repair works on files no slot holds: closing a slot does not
        // concern it.
        if let Some(job) = self.flow.list_job.as_mut() {
            job.target_path = None;
        }
        self.maintenance.running = Some(RunningJob {
            request,
            kind: MaintJobKind::Repair,
            slot: id,
            detached: true,
            what: JobWhat::Repair,
            discarded: false,
        });
        self.maintenance.backups.clear();
    }

    // ---- the answer of a job ----

    /// Puts the database a job read again in its slot. Returns whether the
    /// slot still holds the database the job worked on.
    fn maint_install(
        &mut self,
        id: SlotId,
        outcome: &MaintOutcome,
        released: bool,
    ) -> Option<bool> {
        let slot = self.bases.slot_mut(id)?;
        if outcome.in_memory {
            if outcome.changed {
                slot.data_changed();
                // The Clipbase keeps its identity: the windows that follow it
                // see the version.
                self.spelling.version = self.spelling.version.wrapping_add(1);
            }
            return Some(true);
        }
        match &outcome.source {
            Some(source) => {
                let total = GameSource::len(&*source.read());
                slot.source = source.clone();
                slot.stamp = outcome.stamp;
                if outcome.changed {
                    slot.data_changed();
                }
                if total != slot.filter.len() {
                    slot.filter = Filter::new(total);
                    slot.list.select_only(None);
                    slot.clear_sort();
                    return Some(false);
                }
                Some(true)
            }
            None if released => {
                // The reader was let go and the files cannot be read again:
                // the slot is empty and refuses everything until it is
                // closed and opened.
                slot.needs_reload = true;
                slot.filter = Filter::new(0);
                slot.list.select_only(None);
                slot.current_game = None;
                slot.clear_sort();
                Some(false)
            }
            None => Some(true),
        }
    }

    fn maint_error_notice(what: &JobWhat, error: BaseError) -> MaintNotice {
        match (what, error) {
            (JobWhat::TagEdit | JobWhat::TagFilter, BaseError::Unencodable) => {
                MaintNotice::InvalidValue
            }
            (JobWhat::Repair, BaseError::AlreadyOpen) => MaintNotice::RepairOpen,
            (_, error) => MaintNotice::Failed(error),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat decision per kind of job
    fn maint_done(
        &mut self,
        request: RequestId,
        result: Result<MaintOutcome, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(job) = self.flow.list_job.take_if(|j| j.request == request) else {
            // The answer of a job that is not the one in flight.
            return;
        };
        let Some(run) = self.maintenance.running.take_if(|r| r.request == request) else {
            return;
        };
        let writes = run.what.writes();
        if writes {
            self.reads_unblock();
        }
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                if writes
                    && error == BaseError::Internal
                    && let Some(slot) = self.bases.slot_mut(run.slot)
                    && slot.source == job.source
                {
                    // The task died: the reader may have been let go.
                    slot.needs_reload = true;
                }
                if error != BaseError::Cancelled {
                    let notice = Self::maint_error_notice(&run.what, error);
                    self.maint_notice(notice);
                }
                return;
            }
        };
        self.maintenance.backups.clone_from(&outcome.backups);
        let released = writes && !outcome.in_memory;
        if run.detached {
            self.maint_done_detached(&run, &outcome);
            return;
        }
        let holds = self
            .bases
            .slot(run.slot)
            .is_some_and(|s| s.source == job.source);
        if !holds || job.closing {
            // The slot was closed or holds another database now: there is
            // nothing to update, but what was written is told.
            if outcome.changed {
                self.maint_notice(MaintNotice::Interrupted);
            }
            return;
        }
        let kept = self
            .maint_install(run.slot, &outcome, released)
            .unwrap_or(false);
        let stopped = outcome.error == Some(BaseError::Cancelled);
        let failed = outcome.error.clone().filter(|e| *e != BaseError::Cancelled);
        let had_current = self.bases.slot(run.slot).and_then(|s| s.current_game);
        let report = outcome.report.clone();
        match (&run.what, report) {
            (JobWhat::Flags { flag }, MaintReport::Flags { changed }) => {
                self.after_flags(run.slot, *flag == MaintFlag::Delete, &outcome);
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                } else {
                    self.maint_notice(MaintNotice::FlagsDone { changed });
                }
            }
            (JobWhat::Header, _) => {
                self.tree_cache_rebind(run.slot);
                self.maint_twins_rebind(run.slot);
                if failed.is_none() {
                    self.maint_notice(MaintNotice::HeaderDone);
                }
            }
            (JobWhat::DropDeleted, MaintReport::FilterWithoutDeleted { filter, removed }) => {
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                } else if let Some(slot) = self.bases.slot_mut(run.slot)
                    && slot.filter.len() == filter.len()
                {
                    slot.filter = (*filter).clone();
                    slot.filter_changed();
                    self.maint_notice(MaintNotice::FilterWithoutDeleted { removed });
                } else {
                    self.maint_notice(MaintNotice::Failed(BaseError::Stale));
                }
            }
            (JobWhat::Twins, MaintReport::Twins(scan)) => {
                self.twins_found(run.slot, scan, stopped);
            }
            (JobWhat::UndeleteFirst, MaintReport::Marked { undeleted, .. }) => {
                self.after_flags(run.slot, true, &outcome);
                if failed.is_none() && !stopped {
                    self.maintenance.undeleted_first = undeleted;
                    self.maint_find_twins(run.slot, effects);
                } else if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                }
            }
            (JobWhat::UndeleteAll, MaintReport::Marked { undeleted, .. }) => {
                self.after_flags(run.slot, true, &outcome);
                self.maint_notice(MaintNotice::FlagsDone { changed: undeleted });
            }
            (JobWhat::MarkTwins, MaintReport::Marked { marked, .. }) => {
                self.after_marked(run.slot, marked, &outcome);
            }
            (JobWhat::ShareTags, MaintReport::TagsShared { count }) => {
                self.maint_twins_rebind(run.slot);
                if let Some(checker) = self.maintenance.checker.as_mut() {
                    checker.share = None;
                }
                self.checker_request_diff(effects);
                self.maint_notice(MaintNotice::TagsShared { count });
            }
            (JobWhat::Check, MaintReport::Check(report)) => {
                let notice = if report.problems == 0 && !report.interrupted {
                    MaintNotice::CheckClean {
                        examined: report.examined,
                    }
                } else if report.interrupted {
                    MaintNotice::Interrupted
                } else {
                    MaintNotice::CheckProblems {
                        problems: report.problems,
                    }
                };
                self.maintenance.check = Some(report);
                self.maint_notice(notice);
            }
            (JobWhat::Compact, MaintReport::Compact { games, names }) => {
                let removed = games.map_or(0, |g| g.deleted.saturating_add(g.corrupt));
                let names_removed = names.map_or(0, |n| n.removed.iter().sum());
                self.after_rewrite(run.slot, removed > 0, had_current, &run, effects);
                self.maint_notice(MaintNotice::Compacted {
                    games_removed: removed,
                    names_removed,
                });
            }
            (
                JobWhat::Sort,
                MaintReport::Sort {
                    moved,
                    order,
                    compacted,
                    ..
                },
            ) => {
                let removed = compacted.map_or(0, |g| g.deleted.saturating_add(g.corrupt));
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                } else if removed > 0 || !kept {
                    self.after_rewrite(run.slot, true, had_current, &run, effects);
                    self.maint_notice(MaintNotice::Sorted { moved });
                } else {
                    self.after_sort(run.slot, &order);
                    self.maint_notice(MaintNotice::Sorted { moved });
                }
            }
            (JobWhat::Strip, MaintReport::Rewrite(report)) => {
                self.after_rewrite(run.slot, false, had_current, &run, effects);
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                } else {
                    self.maint_notice(MaintNotice::Stripped {
                        changed: report.changed,
                        skipped: report.skipped.saturating_add(report.unreadable),
                    });
                }
            }
            (JobWhat::TagEdit, MaintReport::Rewrite(report)) => {
                self.after_rewrite(run.slot, false, had_current, &run, effects);
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                } else {
                    self.maint_notice(MaintNotice::TagsDone {
                        changed: report.changed,
                        skipped: report.skipped.saturating_add(report.unreadable),
                    });
                }
                // The listing is of the old games.
                self.maintenance.tags = None;
            }
            (JobWhat::TagList { scope }, MaintReport::TagList(listing)) => {
                self.maintenance.tags = Some(crate::maintenance::TagListView {
                    listing,
                    scope: *scope,
                });
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                }
            }
            (JobWhat::TagFilter, MaintReport::TagFilter { filter, matched }) => {
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                } else if let Some(slot) = self.bases.slot_mut(run.slot)
                    && slot.filter.len() == filter.len()
                {
                    slot.filter = (*filter).clone();
                    slot.filter_changed();
                    slot.list.first = 1;
                    self.maint_notice(MaintNotice::TagFiltered { matched });
                } else {
                    self.maint_notice(MaintNotice::Failed(BaseError::Stale));
                }
            }
            (JobWhat::Names, MaintReport::Names(report)) => {
                self.after_rewrite(run.slot, false, had_current, &run, effects);
                if stopped {
                    self.maint_notice(MaintNotice::Interrupted);
                } else {
                    self.maint_notice(MaintNotice::NamesDone {
                        games: report.games_changed,
                        edits: report.edits,
                    });
                }
            }
            (JobWhat::Names, MaintReport::NamesRefused(_)) => {
                self.after_rewrite(run.slot, false, had_current, &run, effects);
                self.maint_notice(MaintNotice::NamesRefused);
            }
            (JobWhat::Cleaner, MaintReport::Cleaner(report)) => {
                let compacted = report
                    .record(mychess_db::maintenance::CleanerStep::CompactGames)
                    .is_some_and(|r| r.counts.games_removed > 0);
                self.after_rewrite(run.slot, compacted, had_current, &run, effects);
                if outcome_primed(&outcome) {
                    // The `.stc` was written by the last step: the cache of
                    // the slot reads it again instead of removing it.
                    self.tree_cache_forget(run.slot);
                }
                let failed_steps = report
                    .records()
                    .iter()
                    .filter(|r| r.status == mychess_db::maintenance::StepStatus::Failed)
                    .count();
                self.maintenance.last = Some(MaintReport::Cleaner(report));
                self.maint_notice(MaintNotice::CleanerDone {
                    failed: u32::try_from(failed_steps).unwrap_or(u32::MAX),
                });
                return self.maint_done_error(&run, outcome.error);
            }
            _ => {}
        }
        if !matches!(run.what, JobWhat::Cleaner) {
            self.maintenance.last = Some(outcome.report);
        }
        if let Some(error) = failed {
            let notice = Self::maint_error_notice(&run.what, error);
            self.maint_notice(notice);
        }
    }

    /// The error that ended the Cleaner, after its own notice.
    fn maint_done_error(&mut self, run: &RunningJob, error: Option<BaseError>) {
        if let Some(error) = error.filter(|e| *e != BaseError::Cancelled && *e != BaseError::Stale)
        {
            let notice = Self::maint_error_notice(&run.what, error);
            self.maint_notice(notice);
        }
    }

    fn maint_done_detached(&mut self, run: &RunningJob, outcome: &MaintOutcome) {
        let backups = outcome.backups.clone();
        match (&outcome.report, &outcome.error) {
            (MaintReport::Repair(report), None) => {
                self.maint_notice(MaintNotice::Repaired {
                    games: report.games,
                    backups,
                });
            }
            (_, Some(BaseError::Cancelled)) => self.maint_notice(MaintNotice::Interrupted),
            (_, Some(BaseError::AlreadyOpen)) => self.maint_notice(MaintNotice::RepairOpen),
            (_, Some(_)) => self.maint_notice(MaintNotice::RepairFailed { backups }),
            _ => {}
        }
        self.maintenance.last = Some(outcome.report.clone());
        let _ = run;
    }

    /// What follows a change of flags.
    fn after_flags(&mut self, id: SlotId, deleted: bool, outcome: &MaintOutcome) {
        if !deleted && !outcome.in_memory {
            // A flag of marking does not change the trees (D3534).
            self.tree_cache_rebind(id);
        }
        if let Some(slot) = self.bases.slot_mut(id) {
            if outcome.changed {
                slot.data_changed();
            }
            if slot
                .list
                .sort
                .is_some_and(|(c, _)| c == crate::bases::ListColumn::Deleted)
            {
                slot.clear_sort();
            }
        }
        self.maint_twins_rebind(id);
        self.refresh_browser_flags(id);
    }

    /// The games the duplicates took are marked.
    fn after_marked(&mut self, id: SlotId, marked: u32, outcome: &MaintOutcome) {
        self.after_flags(id, true, outcome);
        let set_filter = self.prefs.maintenance.twins.set_filter;
        let to_delete: Vec<u32> = self
            .maintenance
            .twins
            .as_ref()
            .map(|t| t.scan.to_delete.clone())
            .unwrap_or_default();
        if set_filter
            && !to_delete.is_empty()
            && let Some(slot) = self.bases.slot_mut(id)
        {
            // The filter is exactly the games marked by this run (rule 21).
            let mut filter = Filter::new(slot.total());
            filter.fill(0);
            for number in &to_delete {
                filter.set(*number, 1);
            }
            slot.filter = filter;
            slot.filter_changed();
            slot.list.first = 1;
        }
        if let Some(twins) = self.maintenance.twins.as_mut() {
            twins.marked = true;
        }
        let undeleted = std::mem::take(&mut self.maintenance.undeleted_first);
        self.maint_notice(MaintNotice::TwinsMarked { marked, undeleted });
    }

    /// What follows an operation that rewrote the games: the sorted view is
    /// stale, the windows that follow the games are told, and the game on
    /// the editor is read again (or is a new one when the numbers changed or
    /// the user discarded it).
    fn after_rewrite(
        &mut self,
        id: SlotId,
        renumbered: bool,
        had_current: Option<u32>,
        run: &RunningJob,
        effects: &mut Vec<Effect>,
    ) {
        let Some(slot) = self.bases.slot_mut(id) else {
            return;
        };
        slot.data_changed();
        slot.clear_sort();
        if renumbered {
            // The numbers mean nothing now: the filter is "everything" again
            // and the game of the editor is a new one (Scid: `sc_game new`).
            slot.filter = Filter::new(slot.total());
            slot.list.select_only(None);
            slot.list.first = 1;
            slot.current_game = None;
            slot.parked = None;
            self.merge.slot_gone(id);
            if self.browser.as_ref().is_some_and(|b| b.slot == id) {
                self.close_browser();
            }
            if self.flow.base_game.as_ref().is_some_and(|g| g.slot == id) {
                self.flow.base_game = None;
            }
        }
        self.maintenance.twins = None;
        self.maintenance.checker = None;
        if id != self.bases.current() {
            return;
        }
        if renumbered && had_current.is_some() || run.discarded {
            self.apply_replacement(Replacement::New);
        } else if let Some(number) = had_current
            && self.names_reload_allowed()
        {
            self.request_base_game(id, number, None, effects);
            if let Some(pending) = self.flow.base_game.as_mut() {
                pending.names_refresh = true;
            }
        }
    }

    /// Carries the filter, the selection and the game of the editor through
    /// the permutation of a sort (spec rule 41).
    fn after_sort(&mut self, id: SlotId, order: &Shared<Vec<u32>>) {
        use crate::maintenance::permute::{new_number, permute_filter};
        let Some(slot) = self.bases.slot_mut(id) else {
            return;
        };
        let moved_filter = permute_filter(&slot.filter, order);
        let Some(filter) = moved_filter else {
            // Not the same games: the numbers mean nothing.
            slot.filter = Filter::new(slot.total());
            slot.list.select_only(None);
            slot.current_game = None;
            slot.clear_sort();
            self.bases_notice_reset();
            return;
        };
        slot.filter = filter;
        let selection: std::collections::BTreeSet<u32> = slot
            .list
            .selection
            .iter()
            .filter_map(|n| new_number(order, *n))
            .collect();
        slot.list.selected = slot.list.selected.and_then(|n| new_number(order, n));
        slot.list.selection = selection;
        slot.current_game = slot.current_game.and_then(|n| new_number(order, n));
        slot.clear_sort();
        slot.filter_changed();
        // The browser and a game being read hold the old numbers.
        if self.browser.as_ref().is_some_and(|b| b.slot == id) {
            self.close_browser();
        }
        if self.flow.base_game.as_ref().is_some_and(|g| g.slot == id) {
            self.flow.base_game = None;
        }
        self.merge.slot_gone(id);
        self.maintenance.twins = None;
        self.maintenance.checker = None;
        self.maintenance.tags = None;
    }

    fn bases_notice_reset(&mut self) {
        self.notice = Some(AppNotice::Bases(BasesNotice::ListReset));
    }

    fn maint_twins_rebind(&mut self, id: SlotId) {
        let Some((source, rev)) = self
            .bases
            .slot(id)
            .map(|s| (s.source.clone(), s.data_rev()))
        else {
            return;
        };
        if let Some(twins) = self.maintenance.twins.as_mut().filter(|t| t.slot == id) {
            twins.source = source;
            twins.data_rev = rev;
        }
    }

    // ---- duplicates ----

    /// The scan ended: the pairs are kept for the checker and, when
    /// something would be deleted, the question with the count comes.
    fn twins_found(&mut self, id: SlotId, scan: Shared<mychess_db::DuplicateScan>, stopped: bool) {
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let source = slot.source.clone();
        let undeleted = std::mem::take(&mut self.maintenance.undeleted_first);
        self.maintenance.twins = Some(TwinState {
            slot: id,
            source: source.clone(),
            data_rev: slot.data_rev(),
            scan: scan.clone(),
            marked: false,
        });
        self.maintenance.checker = None;
        if stopped {
            self.maint_notice(MaintNotice::Interrupted);
            return;
        }
        let marked = scan.report.marked;
        if scan.to_delete.is_empty() {
            self.maint_notice(if scan.pairs.is_empty() {
                MaintNotice::TwinsNone
            } else {
                MaintNotice::TwinsFound {
                    marked,
                    protected: scan.report.protected,
                }
            });
            return;
        }
        self.maintenance.undeleted_first = undeleted;
        let copy_ratings = self.prefs.maintenance.twins.copy_ratings;
        let prepared = Prepared {
            kind: MaintJobKind::Twins,
            what: JobWhat::MarkTwins,
            op: MaintOp::MarkTwins { scan, copy_ratings },
            gate: Gate::Confirm(ConfirmKind::MarkTwins),
            rewrites: false,
            count: marked,
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        let confirm = confirm_view(&prepared);
        self.maintenance.staged = Some(Staged {
            slot: id,
            source,
            confirm: Some(confirm),
            prepared,
            stage: Stage::Confirming,
            discarded: false,
        });
    }

    /// After the undelete that came first: the scan, with the criteria of
    /// the preferences.
    fn maint_find_twins(&mut self, id: SlotId, effects: &mut Vec<Effect>) {
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let twins = &self.prefs.maintenance.twins;
        let prepared = Prepared {
            kind: MaintJobKind::Twins,
            what: JobWhat::Twins,
            op: MaintOp::Twins {
                criteria: twins.criteria(),
                filter: twins.only_filter.then(|| Box::new(slot.filter.clone())),
            },
            gate: Gate::Open,
            rewrites: false,
            count: 0,
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        let staged = Staged {
            slot: id,
            source: slot.source.clone(),
            confirm: None,
            prepared,
            stage: Stage::Confirming,
            discarded: false,
        };
        self.maint_launch(staged, effects);
    }

    // ---- the checker of duplicates ----

    fn checker_open(&mut self, effects: &mut Vec<Effect>) {
        let has_pairs = self.maint_twins().is_some_and(|t| !t.scan.pairs.is_empty());
        if !has_pairs {
            self.maint_notice(MaintNotice::NoPair);
            return;
        }
        self.maintenance.checker = Some(Checker::default());
        self.checker_request_diff(effects);
    }

    fn checker_step(&mut self, direction: i8, effects: &mut Vec<Effect>) {
        let total = self.maint_twins().map_or(0, |t| t.scan.pairs.len());
        let Some(checker) = self.maintenance.checker.as_mut() else {
            return;
        };
        if total == 0 {
            self.maint_notice(MaintNotice::NoPair);
            return;
        }
        checker.index = if direction >= 0 {
            (checker.index + 1).min(total - 1)
        } else {
            checker.index.saturating_sub(1)
        };
        checker.diff = None;
        checker.share = None;
        self.checker_request_diff(effects);
    }

    /// The slot and the two games of the pair the checker shows.
    fn checker_pair(&self) -> Option<(SlotId, (u32, u32))> {
        let twins = self.maint_twins()?;
        let checker = self.maintenance.checker.as_ref()?;
        let pair = twins.scan.pairs.get(checker.index)?;
        Some((twins.slot, (pair.first, pair.second)))
    }

    fn checker_request_diff(&mut self, effects: &mut Vec<Effect>) {
        let Some((id, pair)) = self.checker_pair() else {
            return;
        };
        let Some(source) = self.bases.slot(id).map(|s| s.source.clone()) else {
            return;
        };
        let request = self.new_request();
        if let Some(checker) = self.maintenance.checker.as_mut() {
            checker.diff_request = Some(request);
            checker.diff = None;
        }
        effects.push(Effect::Maintenance(MaintenanceEffect::Diff {
            request,
            source,
            games: pair,
        }));
    }

    fn checker_toggle(&mut self, sides: &[TwinSide], effects: &mut Vec<Effect>) {
        let Some((id, pair)) = self.checker_pair() else {
            self.maint_notice(MaintNotice::NoPair);
            return;
        };
        let numbers: Vec<u32> = sides
            .iter()
            .map(|side| match side {
                TwinSide::Left => pair.0,
                TwinSide::Right => pair.1,
            })
            .collect();
        if let Some(notice) = self.maint_refusal(id, Need::Editable) {
            self.maint_notice(notice);
            return;
        }
        if self.discard_prompt.is_some() || self.list_busy() {
            return;
        }
        let prepared = Prepared {
            kind: MaintJobKind::Flags,
            what: JobWhat::Flags {
                flag: MaintFlag::Delete,
            },
            op: MaintOp::Flags {
                games: GameSet::Numbers(numbers),
                flag: mychess_db::si4::GameFlags::DELETE,
                action: FlagOp::Toggle,
            },
            gate: Gate::Open,
            rewrites: false,
            count: 0,
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        self.maint_stage(id, prepared, effects);
    }

    fn checker_undelete_all(&mut self) {
        let Some((id, _)) = self.checker_pair() else {
            self.maint_notice(MaintNotice::NoPair);
            return;
        };
        if let Some(notice) = self.maint_refusal(id, Need::Editable) {
            self.maint_notice(notice);
            return;
        }
        if self.discard_prompt.is_some() || self.list_busy() {
            return;
        }
        let Some(source) = self.bases.slot(id).map(|s| s.source.clone()) else {
            return;
        };
        let prepared = Prepared {
            kind: MaintJobKind::Flags,
            what: JobWhat::UndeleteAll,
            op: MaintOp::UndeleteAll,
            gate: Gate::Confirm(ConfirmKind::UndeleteAll),
            rewrites: false,
            count: self.maint_deleted_count(id),
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        self.maintenance.staged = Some(Staged {
            slot: id,
            source,
            confirm: Some(confirm_view(&prepared)),
            prepared,
            stage: Stage::Confirming,
            discarded: false,
        });
    }

    fn checker_share(&mut self, effects: &mut Vec<Effect>) {
        let Some((id, pair)) = self.checker_pair() else {
            self.maint_notice(MaintNotice::NoPair);
            return;
        };
        if let Some(notice) = self.maint_refusal(id, Need::Si4) {
            self.maint_notice(notice);
            return;
        }
        let Some(source) = self.bases.slot(id).map(|s| s.source.clone()) else {
            return;
        };
        let request = self.new_request();
        if let Some(checker) = self.maintenance.checker.as_mut() {
            checker.share_request = Some(request);
        }
        effects.push(Effect::Maintenance(MaintenanceEffect::SharePlan {
            request,
            source,
            games: pair,
        }));
    }

    fn share_planned(
        &mut self,
        request: RequestId,
        tags: Option<Shared<Vec<mychess_db::maintenance::SharedTag>>>,
    ) {
        let Some(checker) = self.maintenance.checker.as_mut() else {
            return;
        };
        if checker.share_request != Some(request) {
            return;
        }
        checker.share_request = None;
        let Some(tags) = tags else {
            self.maint_notice(MaintNotice::Failed(BaseError::Internal));
            return;
        };
        if tags.is_empty() {
            checker.share = None;
            self.maint_notice(MaintNotice::NothingToShare);
            return;
        }
        checker.share = Some(tags.clone());
        let Some((id, pair)) = self.checker_pair() else {
            return;
        };
        let Some(source) = self.bases.slot(id).map(|s| s.source.clone()) else {
            return;
        };
        let prepared = Prepared {
            kind: MaintJobKind::Twins,
            what: JobWhat::ShareTags,
            op: MaintOp::ShareTags { games: pair },
            gate: Gate::Confirm(ConfirmKind::ShareTags),
            rewrites: false,
            count: u32::try_from(tags.len()).unwrap_or(u32::MAX),
            backup_offered: false,
            remember: Remember::Nothing,
            weak: Vec::new(),
        };
        self.maintenance.staged = Some(Staged {
            slot: id,
            source,
            confirm: Some(confirm_view(&prepared)),
            prepared,
            stage: Stage::Confirming,
            discarded: false,
        });
    }
}

fn flag_op(action: FlagAction) -> FlagOp {
    match action {
        FlagAction::Set => FlagOp::Set,
        FlagAction::Clear => FlagOp::Clear,
        FlagAction::Toggle => FlagOp::Toggle,
    }
}

fn confirm_view(prepared: &Prepared) -> ConfirmView {
    let kind = match &prepared.gate {
        Gate::Confirm(kind) | Gate::Preview(_, kind) => *kind,
        Gate::Open => ConfirmKind::Cleaner,
    };
    ConfirmView {
        kind,
        count: prepared.count,
        weak: prepared.weak.clone(),
        backup_offered: prepared.backup_offered,
        backup: false,
    }
}

fn outcome_primed(outcome: &MaintOutcome) -> bool {
    outcome.tree_primed
}
