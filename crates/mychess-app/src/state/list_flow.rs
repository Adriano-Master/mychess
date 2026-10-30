//! The actions of the game list (spec M3-4, slice 4c; D556 onwards):
//! selection, the filter, quick search, sorting, copying to the Clipbase and
//! the "deleted" flag.
//!
//! Everything here is bookkeeping on the slot, which `update` may do. Work
//! that reads the database (searching, sorting, copying, writing flags) is
//! an [`Effect`] run by `perform`; its answer comes back as a
//! [`ListMessage`] and replaces the filter or the order of the slot only
//! when it finishes, so cancelling keeps what was there (D328). One job runs
//! at a time (D561).

use mychess_db::{Filter, GameSource};

use super::{AppState, PendingListJob};
use crate::bases::{
    BaseError, BaseKind, BasesNotice, CopyEnd, CopyList, CopyOutcome, CopyPrompt, CopyRefusal,
    CopyScope, CopyTarget, ListColumn, ListJobKind, ListMessage, OpenControl, PickMode, SearchKind,
    SearchOutcome, SearchSpec, SlotId, SortDir, ViewOrder,
};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::AppNotice;

impl AppState {
    /// The list job in flight, if any: what it does and its progress
    /// (games done and total, from [`OpenControl::progress`]) and the
    /// cancellation.
    ///
    /// A job whose slot was closed (cancelled, still winding down) is not
    /// reported: there is nothing left to show, though the databases stay
    /// busy until its answer arrives (D588).
    pub fn list_job(&self) -> Option<(ListJobKind, &OpenControl)> {
        self.flow
            .list_job
            .as_ref()
            .filter(|j| !j.closing)
            .map(|j| (j.kind, &j.control))
    }

    fn bases_notice(&mut self, notice: BasesNotice) {
        self.notice = Some(AppNotice::Bases(notice));
    }

    /// Whether a list job is running; tells the user when it is.
    pub(crate) fn list_busy(&mut self) -> bool {
        // The batch of annotations owns the .si4 of its slot (D1551).
        if self.flow.annotate_batch.is_some() {
            self.annotate_notice(crate::annotate::AnnotateNotice::Busy);
            return true;
        }
        // A search by position, material or moves rewrites the filter too
        // (spec M6-1).
        if self.flow.list_job.is_some()
            || self.advanced_search_running()
            || self.pattern_search_running()
        {
            self.bases_notice(BasesNotice::ListBusy);
            true
        } else {
            false
        }
    }

    /// Handles a [`ListMessage`].
    pub(crate) fn list_message(&mut self, message: ListMessage, effects: &mut Vec<Effect>) {
        self.list_message_inner(message, effects);
        // A quit waiting for the job goes on when the job is done (D589).
        if self.flow.quit_waiting_job && self.flow.list_job.is_none() {
            self.flow.quit_waiting_job = false;
            self.finish_quit(effects);
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the messages
    fn list_message_inner(&mut self, message: ListMessage, effects: &mut Vec<Effect>) {
        match message {
            // Answers to effects: always handled, even with a dialog open.
            ListMessage::Searched {
                request,
                kind,
                result,
            } => self.list_searched(request, kind, result),
            ListMessage::Sorted { request, result } => self.list_sorted(request, result),
            ListMessage::Flagged { request, result } => self.list_flagged(request, result),
            ListMessage::EcoClassified { request, result } => {
                self.list_eco_classified(request, result);
            }
            ListMessage::PgnImported { request, result } => {
                self.list_pgn_imported(request, result, effects);
            }
            ListMessage::GamesExported { request, result } => {
                self.list_games_exported(request, result, effects);
            }
            ListMessage::EcoNovelty { request, result } => {
                self.list_eco_novelty(request, result, effects);
            }
            ListMessage::GamesCopied { request, result } => self.games_copied(request, result),
            ListMessage::AnswerCopy(yes) => self.answer_copy(yes, effects),
            ListMessage::CancelJob => {
                if let Some(job) = &self.flow.list_job {
                    job.control.cancel();
                }
            }
            _ if self.modal_open() => {}
            ListMessage::Pick { slot, number, mode } => self.list_pick(slot, number, mode),
            ListMessage::SelectNumbers { slot, numbers } => {
                if let Some(slot) = self.bases.slot_mut(slot) {
                    let kept: Vec<u32> = numbers
                        .into_iter()
                        .filter(|n| slot.filter.contains(*n))
                        .collect();
                    slot.list.selection = kept.iter().copied().collect();
                    slot.list.selected = slot
                        .list
                        .selected
                        .filter(|n| slot.list.selection.contains(n))
                        .or_else(|| kept.last().copied());
                }
            }
            ListMessage::SetIgnoreCase { slot, ignore_case } => {
                if let Some(slot) = self.bases.slot_mut(slot) {
                    slot.list.ignore_case = ignore_case;
                }
            }
            ListMessage::Find { slot } => self.list_search(slot, SearchKind::Find, effects),
            ListMessage::FilterByText { slot } => {
                self.list_search(slot, SearchKind::Filter, effects);
            }
            ListMessage::ResetFilter { slot } => {
                self.change_filter(slot, |filter, _| filter.fill(1));
            }
            ListMessage::NegateFilter { slot } => {
                self.change_filter(slot, |filter, _| filter.negate());
            }
            ListMessage::RemoveSelected { slot } => {
                self.change_filter(slot, |filter, state| {
                    for n in &state.list.selection {
                        filter.set(*n, 0);
                    }
                });
            }
            ListMessage::RemoveAbove { slot, number } => {
                self.change_filter(slot, |filter, state| {
                    if let Some(position) = state.position_of(number) {
                        for n in state.visible_range(1, position.saturating_sub(1)) {
                            filter.set(n, 0);
                        }
                    }
                });
            }
            ListMessage::RemoveBelow { slot, number } => {
                self.change_filter(slot, |filter, state| {
                    if let Some(position) = state.position_of(number) {
                        for n in state.visible_range(position.saturating_add(1), u32::MAX) {
                            filter.set(n, 0);
                        }
                    }
                });
            }
            ListMessage::KeepSelected { slot } => self.keep_selected(slot),
            ListMessage::SortBy { slot, column } => self.list_sort(slot, column, effects),
            ListMessage::CopySelected { slot } => self.list_copy(slot, effects),
            ListMessage::CopyGames { from, to, scope } => {
                self.copy_games(from, to, scope, false, effects);
            }
            ListMessage::ToggleDeleted { slot } => self.list_toggle_deleted(slot, effects),
            ListMessage::SetReadOnly { slot, read_only } => {
                if let Some(slot) = self.bases.slot_mut(slot)
                    && slot.kind == BaseKind::Si4
                {
                    // A file that cannot be written stays read-only (D582).
                    slot.read_only = read_only || slot.locked_read_only;
                }
            }
        }
    }

    // ---- selection ----

    fn list_pick(&mut self, id: SlotId, number: u32, mode: PickMode) {
        let Some(slot) = self.bases.slot_mut(id) else {
            return;
        };
        if !slot.filter.contains(number) {
            return;
        }
        match mode {
            PickMode::Only => slot.list.select_only(Some(number)),
            PickMode::Toggle => {
                if slot.list.selection.remove(&number) {
                    slot.list.selected = slot.list.selection.iter().next_back().copied();
                } else {
                    slot.list.selection.insert(number);
                    slot.list.selected = Some(number);
                }
            }
            PickMode::Range => {
                let anchor = slot.list.selected.and_then(|a| slot.position_of(a));
                match (anchor, slot.position_of(number)) {
                    (Some(a), Some(b)) => {
                        let numbers = slot.visible_range(a.min(b), a.max(b));
                        slot.list.selection = numbers.into_iter().collect();
                        // The anchor stays where it was, so a second
                        // Shift+click reshapes the range.
                    }
                    _ => slot.list.select_only(Some(number)),
                }
            }
        }
    }

    // ---- the filter ----

    /// Runs `change` on the filter of a slot and brings the list back in
    /// line (selection, position, sorted view). Refused while a job runs
    /// (the job would overwrite the change when it ends).
    fn change_filter(
        &mut self,
        id: SlotId,
        change: impl FnOnce(&mut Filter, &crate::bases::BaseSlot),
    ) -> bool {
        if self.list_busy() {
            return false;
        }
        let Some(slot) = self.bases.slot_mut(id) else {
            return false;
        };
        // The closure reads the slot (selection, order) while changing a
        // copy of the filter, which then replaces the real one.
        let mut filter = slot.filter.clone();
        change(&mut filter, slot);
        slot.filter = filter;
        slot.filter_changed();
        true
    }

    fn keep_selected(&mut self, id: SlotId) {
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        if slot.list.selection.is_empty() {
            self.bases_notice(BasesNotice::NothingSelected);
            return;
        }
        let applied = self.change_filter(id, |filter, state| {
            let kept: Vec<(u32, u8)> = state
                .list
                .selection
                .iter()
                .map(|n| (*n, filter.get(*n)))
                .collect();
            filter.fill(0);
            for (n, ply) in kept {
                filter.set(n, ply);
            }
        });
        if applied && let Some(slot) = self.bases.slot_mut(id) {
            slot.list.first = 1;
        }
    }

    // ---- quick search ----

    pub(super) fn start_job(
        &mut self,
        id: SlotId,
        kind: ListJobKind,
        make: impl FnOnce(RequestId, OpenControl) -> Effect,
        effects: &mut Vec<Effect>,
    ) {
        let Some((source, path)) = self
            .bases
            .slot(id)
            .map(|s| (s.source.clone(), s.path.clone()))
        else {
            return;
        };
        // A flag job rewrites the file of its slot: while it winds down
        // after the slot was closed, the file must not be opened again
        // (D611, like the target of a copy).
        let target_path = path.filter(|_| {
            matches!(
                kind,
                ListJobKind::Flags
                    | ListJobKind::EcoClassify
                    | ListJobKind::SpellApply
                    | ListJobKind::EloAdd
            )
        });
        let request = self.new_request();
        let control = OpenControl::new();
        self.flow.list_job = Some(PendingListJob {
            request,
            slot: id,
            source,
            kind,
            target: None,
            target_path,
            closing: false,
            control: control.clone(),
        });
        effects.push(make(request, control));
    }

    fn list_search(&mut self, id: SlotId, kind: SearchKind, effects: &mut Vec<Effect>) {
        if self.list_busy() {
            return;
        }
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let text = slot.list.search.clone();
        if text.trim().is_empty() {
            return;
        }
        let from = slot
            .list
            .selected
            .and_then(|n| slot.position_of(n))
            .unwrap_or_else(|| slot.list.first.saturating_sub(1));
        let tail = (kind == SearchKind::Find && slot.order.is_some())
            .then(|| slot.visible_range(from.saturating_add(1), u32::MAX));
        // In the database order the cursor is the rank in the filter; the
        // position of an unsorted view is exactly that.
        let spec = SearchSpec {
            kind,
            text,
            ignore_case: slot.list.ignore_case,
            from,
            tail,
        };
        let source = slot.source.clone();
        let filter = Box::new(slot.filter.clone());
        let job = match kind {
            SearchKind::Find => ListJobKind::Find,
            SearchKind::Filter => ListJobKind::Filter,
        };
        self.start_job(
            id,
            job,
            move |request, control| Effect::SearchList {
                request,
                source,
                filter,
                spec,
                control,
            },
            effects,
        );
    }

    /// Takes the job if `request` is the one in flight and the slot still
    /// holds the database it worked on.
    pub(super) fn finish_job(&mut self, request: RequestId) -> Option<PendingListJob> {
        if self.flow.list_job.as_ref().map(|j| j.request) != Some(request) {
            return None;
        }
        let job = self.flow.list_job.take()?;
        let same = self
            .bases
            .slot(job.slot)
            .is_some_and(|slot| slot.source == job.source);
        same.then_some(job)
    }

    pub(super) fn job_failed(&mut self, error: BaseError) {
        // A cancelled job leaves everything as it was and says nothing.
        if error != BaseError::Cancelled {
            self.bases_notice(BasesNotice::Failed(error));
        }
    }

    fn list_searched(
        &mut self,
        request: RequestId,
        kind: SearchKind,
        result: Result<SearchOutcome, BaseError>,
    ) {
        let Some(job) = self.finish_job(request) else {
            return;
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => return self.job_failed(error),
        };
        let Some(slot) = self.bases.slot_mut(job.slot) else {
            return;
        };
        match (kind, outcome) {
            (SearchKind::Find, SearchOutcome::Found(Some(number))) => {
                slot.list.select_only(Some(number));
                if let Some(position) = slot.position_of(number) {
                    slot.list.first = position;
                    slot.clamp_first();
                }
            }
            (SearchKind::Find, SearchOutcome::Found(None)) => {
                // Scid `FindText`: no hit goes back to the first game.
                slot.list.first = 1;
                self.bases_notice(BasesNotice::NotFound);
            }
            (
                SearchKind::Filter,
                SearchOutcome::Filtered {
                    filter,
                    removed,
                    unreadable,
                },
            ) => {
                if filter.len() != slot.filter.len() {
                    self.bases_notice(BasesNotice::Stale);
                    return;
                }
                slot.filter = *filter;
                slot.filter_changed();
                slot.list.first = 1;
                let notice = if unreadable > 0 {
                    BasesNotice::UnreadableEntries { count: unreadable }
                } else {
                    BasesNotice::Filtered { removed }
                };
                self.bases_notice(notice);
            }
            _ => {}
        }
    }

    // ---- sorting ----

    fn list_sort(&mut self, id: SlotId, column: ListColumn, effects: &mut Vec<Effect>) {
        if !column.is_sortable() || self.list_busy() {
            return;
        }
        let Some(slot) = self.bases.slot_mut(id) else {
            return;
        };
        if let Some((current, dir)) = slot.list.sort
            && current == column
            && slot.order.is_some()
        {
            // Same column again: flip the direction. The order of the games
            // is already there; only the cached view is rebuilt.
            let dir = dir.flipped();
            if let Some(order) = &mut slot.order {
                order.set_dir(dir, &slot.filter);
            }
            slot.list.sort = Some((column, dir));
            slot.list.first = 1;
            return;
        }
        let source = slot.source.clone();
        self.start_job(
            id,
            ListJobKind::Sort(column),
            move |request, control| Effect::SortList {
                request,
                source,
                column,
                control,
            },
            effects,
        );
    }

    fn list_sorted(
        &mut self,
        request: RequestId,
        result: Result<std::sync::Arc<Vec<u32>>, BaseError>,
    ) {
        let Some(job) = self.finish_job(request) else {
            return;
        };
        let all = match result {
            Ok(all) => all,
            Err(error) => return self.job_failed(error),
        };
        let ListJobKind::Sort(column) = job.kind else {
            return;
        };
        let Some(slot) = self.bases.slot_mut(job.slot) else {
            return;
        };
        if usize::try_from(slot.filter.len()).ok() != Some(all.len()) {
            self.bases_notice(BasesNotice::Stale);
            return;
        }
        slot.order = Some(ViewOrder::new(SortDir::Ascending, all, &slot.filter));
        slot.list.sort = Some((column, SortDir::Ascending));
        slot.list.first = 1;
    }

    // ---- copying to the Clipbase ----

    /// "Copy to the Clipbase": the selected games, by the same job as the
    /// copy between databases (D588), so the flags of the source go along
    /// (the Scid copies the whole index entry on both paths).
    fn list_copy(&mut self, id: SlotId, effects: &mut Vec<Effect>) {
        if id.is_clipbase() {
            self.bases_notice(BasesNotice::AlreadyInClipbase);
            return;
        }
        if self.list_busy() {
            return;
        }
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        if slot.list.selection.is_empty() {
            self.bases_notice(BasesNotice::NothingSelected);
            return;
        }
        self.start_copy(
            id,
            SlotId::CLIPBASE,
            CopyScope::Selection,
            ListJobKind::Copy,
            effects,
        );
    }

    /// Makes the filter of the Clipbase as long as the Clipbase after games
    /// were added behind its back (the new ones are in, like `append(1)`),
    /// and drops its sorted view.
    pub(crate) fn sync_clipbase_slot(&mut self) {
        let Some(slot) = self.bases.slot_mut(SlotId::CLIPBASE) else {
            return;
        };
        let total = GameSource::len(&*slot.source.read());
        if slot.filter.len() > total {
            slot.filter = Filter::new(total);
        }
        while slot.filter.len() < total {
            if !slot.filter.append(1) {
                break;
            }
        }
        slot.clear_sort();
        slot.clamp_first();
    }

    // ---- copying between databases (slice 4d) ----

    /// The name of a slot for the user.
    fn copy_end(&self, id: SlotId) -> CopyEnd {
        let name = self
            .bases
            .slot(id)
            .and_then(|slot| slot.path.as_deref())
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        CopyEnd { slot: id, name }
    }

    /// Why a copy from `from` to `to` cannot be made, if it cannot. The
    /// Scid (`copyFilter`) tests, in this order, "no games", "read-only
    /// target", "target not open", "source not open", "same database", and
    /// each test overwrites the message of the previous one: the last test
    /// that fails is the one the user sees. The order below is the
    /// reverse, so the first test that fails here is that one (D576).
    fn copy_refusal(&self, from: SlotId, to: SlotId, scope: CopyScope) -> Option<CopyRefusal> {
        if from == to {
            return Some(CopyRefusal::SameBase);
        }
        let Some(source) = self.bases.slot(from) else {
            return Some(CopyRefusal::SourceNotOpen);
        };
        let Some(target) = self.bases.slot(to) else {
            return Some(CopyRefusal::TargetNotOpen);
        };
        if target.read_only || target.needs_reload || target.kind == BaseKind::Pgn {
            return Some(CopyRefusal::TargetReadOnly);
        }
        match scope {
            CopyScope::Filter if source.filtered() == 0 => Some(CopyRefusal::EmptyFilter),
            CopyScope::Selection if source.list.selection.is_empty() => {
                Some(CopyRefusal::NothingSelected)
            }
            _ => None,
        }
    }

    /// Copies the filter (or the selection) of `from` to the end of `to`.
    /// `confirmed`: the user already answered the question.
    fn copy_games(
        &mut self,
        from: SlotId,
        to: SlotId,
        scope: CopyScope,
        confirmed: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.list_busy() {
            return;
        }
        if let Some(reason) = self.copy_refusal(from, to, scope) {
            let (from, to) = (self.copy_end(from), self.copy_end(to));
            self.bases_notice(BasesNotice::CopyRefused { reason, from, to });
            return;
        }
        let Some(source) = self.bases.slot(from) else {
            return;
        };
        let count = match scope {
            CopyScope::Filter => source.filtered(),
            CopyScope::Selection => u32::try_from(source.list.selection.len()).unwrap_or(u32::MAX),
        };
        // Copying to the Clipbase never asks (Scid `copyFilter`).
        if !confirmed && self.prefs.editing.confirm_copy && !to.is_clipbase() {
            self.copy_prompt = Some(CopyPrompt {
                from: self.copy_end(from),
                to: self.copy_end(to),
                scope,
                count,
            });
            return;
        }
        self.start_copy(from, to, scope, ListJobKind::CopyGames, effects);
    }

    /// Builds and starts the copy job (the checks were made by the caller).
    fn start_copy(
        &mut self,
        from: SlotId,
        to: SlotId,
        scope: CopyScope,
        kind: ListJobKind,
        effects: &mut Vec<Effect>,
    ) {
        let Some(source) = self.bases.slot(from) else {
            return;
        };
        let list = match scope {
            CopyScope::Filter => CopyList::Filter(Box::new(source.filter.clone())),
            CopyScope::Selection => {
                CopyList::Numbers(source.list.selection.iter().copied().collect())
            }
        };
        self.start_copy_list(from, to, list, kind, effects);
    }

    /// Starts the copy job of `list` (the checks were made by the caller).
    pub(crate) fn start_copy_list(
        &mut self,
        from: SlotId,
        to: SlotId,
        list: CopyList,
        kind: ListJobKind,
        effects: &mut Vec<Effect>,
    ) {
        let (Some(source), Some(target)) = (self.bases.slot(from), self.bases.slot(to)) else {
            return;
        };
        let target_path = target.path.clone();
        let write_to = if to.is_clipbase() {
            CopyTarget::Clipbase(target.source.clone())
        } else if let Some(path) = target_path.clone() {
            CopyTarget::Si4 {
                path,
                expected_total: target.total(),
                stamp: target.stamp,
            }
        } else {
            self.bases_notice(BasesNotice::Failed(BaseError::Internal));
            return;
        };
        // An `.si4` source must still be the file that was read (D590).
        let source_stamp = source
            .path
            .clone()
            .filter(|_| source.kind == BaseKind::Si4)
            .zip(source.stamp);
        let (from_source, to_source) = (source.source.clone(), target.source.clone());
        let request = self.new_request();
        let control = OpenControl::new();
        self.flow.list_job = Some(PendingListJob {
            request,
            slot: from,
            source: from_source.clone(),
            kind,
            target: Some((to, to_source)),
            target_path,
            closing: false,
            control: control.clone(),
        });
        effects.push(Effect::CopyGames {
            request,
            source: from_source,
            source_stamp,
            list,
            target: write_to,
            control,
        });
    }

    /// The answer to "copy N games?": the copy is checked again, because
    /// the databases may have changed while the question was open.
    fn answer_copy(&mut self, yes: bool, effects: &mut Vec<Effect>) {
        let Some(prompt) = self.copy_prompt.take() else {
            return;
        };
        if yes {
            self.copy_games(
                prompt.from.slot,
                prompt.to.slot,
                prompt.scope,
                true,
                effects,
            );
        }
    }

    #[allow(clippy::too_many_lines)] // one flat decision on the outcome
    fn games_copied(&mut self, request: RequestId, result: Result<CopyOutcome, BaseError>) {
        if self.flow.list_job.as_ref().map(|j| j.request) != Some(request) {
            return;
        }
        let Some(job) = self.flow.list_job.take() else {
            return;
        };
        let (from, Some((to, target_source))) = (self.copy_end(job.slot), job.target.clone())
        else {
            return;
        };
        let to_end = self.copy_end(to);
        let selected_copy = job.kind == ListJobKind::Copy;
        let outcome = match result {
            Ok(outcome) => outcome,
            // Nothing was written. A copy of the selection to the Clipbase
            // keeps the notices of slice 4c; a cancel is silent when the
            // user closed a database (D588).
            Err(error) => {
                if error == BaseError::Cancelled && (job.closing || selected_copy) {
                    return;
                }
                if selected_copy {
                    return self.job_failed(error);
                }
                let notice = BasesNotice::CopyStopped {
                    count: 0,
                    error,
                    from,
                    to: to_end,
                };
                return self.bases_notice(notice);
            }
        };
        // The target slot grows by the games written, if it still holds
        // the database the copy wrote to.
        let same = self
            .bases
            .slot(to)
            .is_some_and(|slot| slot.source == target_source);
        let mut reset = false;
        let mut error = outcome.error.clone();
        if same {
            if to.is_clipbase() {
                self.sync_clipbase_slot();
            } else if let Some(slot) = self.bases.slot_mut(to) {
                if let Some(reloaded) = outcome.reloaded {
                    let total = GameSource::len(&*reloaded.source.read());
                    let old_total = slot.filter.len();
                    slot.source = reloaded.source;
                    slot.stamp = reloaded.stamp;
                    let mut grown = total == old_total.saturating_add(outcome.copied);
                    if grown {
                        // The new games enter the filter, like `append(1)`
                        // of the Scid; filter, selection and position stay.
                        // The sorted view knows nothing of them.
                        grown = (0..outcome.copied).all(|_| slot.filter.append(1));
                        if outcome.copied > 0 {
                            slot.clear_sort();
                        }
                    }
                    if !grown {
                        // Another number of games than expected: the file
                        // changed behind our back (D582).
                        slot.filter = Filter::new(total);
                        slot.list.select_only(None);
                        slot.clear_sort();
                        reset = true;
                    }
                    slot.clamp_first();
                } else if outcome.copied > 0 {
                    // The games are on disk but the database could not be
                    // opened again: the slot shows the old file and refuses
                    // writes until it is closed and opened (D591).
                    slot.needs_reload = true;
                }
            }
        }
        // A job whose slot was closed ends with a "stopped" remark when
        // something was written, and silently when nothing was.
        if job.closing {
            if outcome.copied == 0 {
                return;
            }
            error = error.or(Some(BaseError::Cancelled));
        }
        if selected_copy && error.is_none() {
            return self.bases_notice(BasesNotice::Copied {
                count: outcome.copied,
            });
        }
        if selected_copy && error == Some(BaseError::Cancelled) && outcome.copied == 0 {
            return;
        }
        let notice = match error {
            None if reset || outcome.truncated > 0 => BasesNotice::FilterCopiedNotes {
                count: outcome.copied,
                from,
                to: to_end,
                truncated: outcome.truncated,
                reset,
            },
            None => BasesNotice::FilterCopied {
                count: outcome.copied,
                from,
                to: to_end,
            },
            // A cancel that already copied some games says how many (D612).
            Some(error) if selected_copy && error != BaseError::Cancelled => {
                BasesNotice::Failed(error)
            }
            Some(error) => BasesNotice::CopyStopped {
                count: outcome.copied,
                error,
                from,
                to: to_end,
            },
        };
        self.bases_notice(notice);
    }

    // ---- flags ----

    pub(crate) fn list_toggle_deleted(&mut self, id: SlotId, effects: &mut Vec<Effect>) {
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        // D560: only an SI4 database can be flagged; a PGN has no file to
        // write and the Clipbase keeps no flags.
        if slot.kind != BaseKind::Si4 || slot.read_only || slot.needs_reload {
            self.bases_notice(BasesNotice::ReadOnly);
            return;
        }
        let Some(path) = slot.path.clone() else {
            self.bases_notice(BasesNotice::ReadOnly);
            return;
        };
        let numbers: Vec<u32> = slot.list.selection.iter().copied().collect();
        if numbers.is_empty() {
            self.bases_notice(BasesNotice::NothingSelected);
            return;
        }
        let (expected_total, stamp) = (slot.total(), slot.stamp);
        if self.list_busy() {
            return;
        }
        self.start_job(
            id,
            ListJobKind::Flags,
            move |request, control| Effect::ToggleDeleted {
                request,
                path,
                numbers,
                expected_total,
                stamp,
                control,
            },
            effects,
        );
    }

    fn list_flagged(
        &mut self,
        request: RequestId,
        result: Result<crate::bases::FlagOutcome, BaseError>,
    ) {
        let Some(job) = self.finish_job(request) else {
            return;
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => return self.job_failed(error),
        };
        let Some(slot) = self.bases.slot_mut(job.slot) else {
            return;
        };
        // The database is read again, whatever happened: the slot shows
        // what is on disk. Filter, selection and position stay.
        let Some(source) = outcome.source else {
            // The file changed but cannot be read: the slot keeps the old
            // database, refuses writes, and says why (D592).
            slot.needs_reload |= outcome.changed > 0;
            if let Some(error) = outcome.error {
                self.job_failed(error);
            }
            return;
        };
        let total = GameSource::len(&*source.read());
        slot.source = source;
        slot.stamp = outcome.stamp;
        let reset = total != slot.filter.len();
        if reset {
            // The file has another number of games than the slot had: the
            // filter, selection and sort no longer mean anything (D560).
            slot.filter = Filter::new(total);
            slot.list.select_only(None);
            slot.clear_sort();
        } else if slot
            .list
            .sort
            .is_some_and(|(c, _)| c == ListColumn::Deleted)
        {
            slot.clear_sort();
        }
        slot.clamp_first();
        // A browser open on the slot shows the new marks.
        self.refresh_browser_flags(job.slot);
        if let Some(error) = outcome.error {
            self.job_failed(error);
        } else if reset {
            self.bases_notice(BasesNotice::ListReset);
        }
    }
}
