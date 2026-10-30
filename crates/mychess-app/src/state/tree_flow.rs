//! The opening tree in the state (spec M5-2, D960-D976).
//!
//! The panel is derived data: at the end of every `update`,
//! [`AppState::refresh_tree`] compares what a search would be asked now (the
//! base, the position of the cursor, the options) with what the last one was
//! asked, and starts one if it changed.
//!
//! # One search in flight (D976)
//!
//! The engine's [`TreeState`] of the slot **moves** into the job and comes
//! back in the answer, so while a search runs the slot has no state to give
//! to another one. A new wish therefore cancels the token, bumps the
//! generation and waits for the (quick) `Cancelled` answer before starting
//! the next search with the state that returns. The alternative, starting a
//! second search at once with a fresh state, would lose the filters the
//! state keeps for "adjust" and for the fast mode. Waiting costs at most the
//! cancellation latency of the engine (a check every 5000 games or 100 ms).

use std::sync::Arc;

use mychess_core::Position;
use mychess_db::Filter;
use mychess_db::progress::CancelToken;
use mychess_db::tree::{FastMode, TreeState};

use super::AppState;
use crate::bases::SlotId;
use crate::effect::Effect;
use crate::message::Message;
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, PlayOptions};
use crate::tree::{
    InFlight, Pending, Phase, RefreshKey, Shown, SlotTree, TreeAnswer, TreeBody, TreeFastMode,
    TreeIssue, TreeJob, TreeMessage, TreePrefs, TreeRowView, TreeTotalView, TreeUi, TreeView,
    bar_segments,
};

impl AppState {
    /// The state of the tree panel.
    pub fn tree(&self) -> &TreeUi {
        &self.tree
    }

    /// The tree panel, ready to draw: numbers only, with the flags the
    /// front-end needs. Cheap (at most 60 rows).
    #[allow(clippy::too_many_lines)] // one flat snapshot of the panel
    pub fn tree_view(&self) -> TreeView {
        let mut view = self.tree_view_base();
        self.mask_decorate(&mut view);
        view
    }

    #[allow(clippy::too_many_lines)] // one flat snapshot of the panel
    fn tree_view_base(&self) -> TreeView {
        let prefs = self.prefs.tree;
        let ui = &self.tree;
        let searching = ui.searching();
        let mut view = TreeView {
            body: TreeBody::Closed,
            rows: Vec::new(),
            total: None,
            turn: mychess_core::Color::White,
            short: prefs.short,
            sort: prefs.sort,
            fast_mode: prefs.fast_mode,
            autorefresh: ui.autorefresh,
            adjust: prefs.auto_adjust,
            searching,
            unreadable: 0,
            from_cache: false,
            searched: 0,
            elapsed_ms: 0,
            training: ui.training,
            training_available: self.tree_training_available(),
            locked: ui.locked.is_some(),
            target_name: None,
            target_is_clipbase: false,
            best: None,
            cache: self.tree_cache_view(),
            mask: None,
        };
        let target = self.tree_target();
        view.target_is_clipbase = target.is_clipbase();
        view.target_name = self
            .bases
            .slot(target)
            .and_then(|slot| slot.path.as_ref())
            .and_then(|path| path.file_stem())
            .map(|name| name.to_string_lossy().into_owned());
        if ui.open {
            view.best = Self::best_view(&ui.best);
        }
        if !ui.open {
            return view;
        }
        if ui.unavailable {
            view.body = TreeBody::Unavailable;
            return view;
        }
        if let Some(issue) = &ui.issue {
            view.body = TreeBody::Issue(issue.clone());
            return view;
        }
        let Some(shown) = &ui.shown else {
            view.body = TreeBody::Loading;
            return view;
        };
        let result = &shown.result;
        view.turn = result.turn;
        view.unreadable = result.unreadable_total;
        view.from_cache = result.from_cache;
        view.searched = result.searched;
        view.elapsed_ms = u64::try_from(result.elapsed.as_millis()).unwrap_or(u64::MAX);
        view.total = result.total.as_ref().map(|t| TreeTotalView {
            games: t.total,
            results: t.freq,
            score: t.score_tenths(),
            draw_percent: t.draw_percent(),
            avg_elo: t.avg_elo,
            perf: t.perf,
            avg_year: t.avg_year,
        });
        if result.rows.is_empty() {
            view.body = TreeBody::Empty;
            return view;
        }
        view.body = TreeBody::Rows;
        let position = self.session.position();
        // The rows describe the position they were searched for; a table
        // that no longer matches the cursor is shown but not interactive.
        let matches = shown.fen == position.fen();
        let next = if matches {
            self.next_main_move(position)
        } else {
            None
        };
        let interactive = matches && !searching && ui.autorefresh;
        let games = view.total.as_ref().map_or(0, |t| t.games);
        view.rows = result
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let is_end = row.is_end();
                let is_next_move = next.is_some_and(|n| {
                    row.uci
                        .as_deref()
                        .and_then(|u| position.parse_uci(u).ok())
                        .is_some_and(|m| m == n)
                });
                TreeRowView {
                    index,
                    san: row.san.clone(),
                    hidden: row.hidden,
                    is_end,
                    clickable: interactive && !is_end && !row.hidden,
                    is_next_move,
                    games: row.total,
                    share: row.share_tenths(games),
                    results: row.freq,
                    score: row.score_tenths(),
                    draw_percent: row.draw_percent(),
                    avg_elo: row.avg_elo,
                    perf: row.perf,
                    avg_year: row.avg_year,
                    eco: row.eco.map(|e| e.to_string()),
                    bar: bar_segments(row.score_permille, row.draw_percent()),
                    mask: None,
                }
            })
            .collect();
        view
    }

    fn best_view(best: &crate::tree::BestUi) -> Option<crate::tree::BestView> {
        best.open.then(|| crate::tree::BestView {
            columns: best.columns.clone(),
            columns_open: best.columns_open,
            results: best.results,
            order: best.order,
            max: best.max,
            rows: Arc::clone(&best.rows),
            selected: best.selected.clone(),
            in_clipbase: best.slot.is_some_and(SlotId::is_clipbase),
            slot: best.slot,
        })
    }

    /// The main-line move after the cursor, if any (D966).
    fn next_main_move(&self, _position: &Position) -> Option<mychess_core::Move> {
        let game = self.session.game();
        let child = game
            .children(self.session.cursor())
            .ok()?
            .first()
            .copied()?;
        game.mv(child).ok()
    }

    pub(crate) fn tree_message(&mut self, message: TreeMessage, effects: &mut Vec<Effect>) {
        match message {
            TreeMessage::Toggle => {
                if self.tree.open {
                    self.tree_cache_autosave_all(effects);
                    self.tree_close();
                } else {
                    self.tree_open();
                }
            }
            TreeMessage::Open => self.tree_open(),
            TreeMessage::Close => {
                if self.tree.open {
                    self.tree_cache_autosave_all(effects);
                }
                self.tree_close();
            }
            TreeMessage::SetSort(sort) => self.set_tree_prefs(
                TreePrefs {
                    sort,
                    ..self.prefs.tree
                },
                effects,
            ),
            TreeMessage::SetFastMode(fast_mode) => {
                self.set_tree_prefs(
                    TreePrefs {
                        fast_mode,
                        ..self.prefs.tree
                    },
                    effects,
                );
            }
            TreeMessage::SetShort(short) => {
                self.set_tree_prefs(
                    TreePrefs {
                        short,
                        ..self.prefs.tree
                    },
                    effects,
                );
            }
            TreeMessage::ToggleAutoRefresh => self.tree.autorefresh = !self.tree.autorefresh,
            TreeMessage::SetAdjust(on) => self.tree_set_adjust(on, effects),
            TreeMessage::RowClicked(index) => self.tree_row_clicked(index, effects),
            TreeMessage::Best(message) => self.tree_best_message(&message, effects),
            TreeMessage::SetTraining(on) => self.tree_set_training(on, effects),
            TreeMessage::SetLocked(on) => self.tree_set_locked(on),
            TreeMessage::OpenBaseAsTree => self.tree_ask_base(effects),
            TreeMessage::Finished(answer) => self.tree_finished(*answer, effects),
            TreeMessage::Mask(message) => self.mask_message(message, effects),
            message @ (TreeMessage::SaveCache
            | TreeMessage::FillCache(_)
            | TreeMessage::CacheInfo
            | TreeMessage::CancelCacheJob
            | TreeMessage::DismissCacheStatus
            | TreeMessage::SetCacheSize(_)
            | TreeMessage::SetCacheAutosave(_)
            | TreeMessage::CacheFinished(_)
            | TreeMessage::CacheDiscarded(_)
            | TreeMessage::CacheDeadline(_)) => self.tree_cache_message(message, effects),
        }
    }

    fn set_tree_prefs(&mut self, prefs: TreePrefs, effects: &mut Vec<Effect>) {
        if prefs != self.prefs.tree {
            self.prefs_message(PrefsMessage::Set(PrefChange::Tree(prefs)), effects);
        }
    }

    pub(super) fn tree_open(&mut self) {
        if self.tree.open {
            return;
        }
        self.tree.open = true;
        self.tree.autorefresh = true;
        self.tree.last_key = None;
        self.tree.issue = None;
    }

    /// Closing ends the tree: the search stops, and a slot whose filter was
    /// adjusted gets the old one back (D967).
    fn tree_close(&mut self) {
        if !self.tree.open {
            return;
        }
        self.tree.open = false;
        self.tree_training_stop(true);
        self.tree.best.clear();
        self.tree.best.open = false;
        self.tree.best.columns_open = false;
        self.tree.cancel_search();
        self.tree.last_key = None;
        self.tree.shown = None;
        self.tree.issue = None;
        self.tree.unavailable = false;
        self.tree.retried = false;
        for index in 0..self.tree.slots.len() {
            if let Some(id) = SlotId::new(index) {
                self.tree_end_entry(id);
            }
        }
    }

    /// `sc_tree clean` for a slot; deferred if its state is out in a search.
    fn tree_end_entry(&mut self, id: SlotId) {
        let Some(entry) = self.tree.entry_mut(id) else {
            return;
        };
        let Some(state) = entry.state.as_mut() else {
            entry.pending = Pending::Clean;
            return;
        };
        let old = state.clean();
        entry.pending = Pending::Nothing;
        if std::mem::take(&mut entry.adjusted) {
            // The engine's copy is the base; if the search never ran (a
            // failure lost the state) the copy made at the first adjust is.
            entry.restore = old.or(entry.original.take());
        }
        entry.original = None;
        self.tree_flush_restore(id);
    }

    /// Gives the slot back the filter it had before the tree adjusted it
    /// (D979, D986), after a failure that lost the engine's copy.
    fn tree_reset_adjusted(&mut self, id: SlotId) {
        let Some(entry) = self.tree.entry_mut(id) else {
            return;
        };
        if std::mem::take(&mut entry.adjusted) {
            entry.restore = entry.original.take();
        }
        entry.original = None;
        self.tree_flush_restore(id);
    }

    /// Applies a restore that was waiting. A running list job defers it
    /// (D987: it would overwrite the job's result, or be overwritten by it).
    /// If the user changed the filter since the tree wrote it, the user's
    /// filter is the new base and stays (Scid: `sc_tree clean ... updateFilter`
    /// copies the main filter to the database filter).
    fn tree_flush_restore(&mut self, id: SlotId) {
        if self.flow.list_job.is_some() {
            return;
        }
        let Some(entry) = self.tree.entry_mut(id) else {
            return;
        };
        let Some(filter) = entry.restore.take() else {
            return;
        };
        let applied = entry.applied.take();
        let same_base = self
            .bases
            .slot(id)
            .is_some_and(|slot| slot.source == entry.source);
        if same_base
            && let Some(slot) = self.bases.slot_mut(id)
            && slot.filter.len() == filter.len()
            && applied.is_none_or(|a| a == slot.filter)
        {
            slot.filter = filter;
            slot.filter_changed();
        }
    }

    /// Runs the restores that waited for a list job.
    fn tree_flush_all_restores(&mut self) {
        if self.flow.list_job.is_some() {
            return;
        }
        for id in SlotId::all() {
            if self
                .tree
                .entry_mut(id)
                .is_some_and(|entry| entry.restore.is_some())
            {
                self.tree_flush_restore(id);
            }
        }
    }

    fn tree_set_adjust(&mut self, on: bool, effects: &mut Vec<Effect>) {
        if on == self.prefs.tree.auto_adjust {
            return;
        }
        let id = self.tree_target();
        // Either way the filter of the slot, as it is now, is the base the
        // tree starts from (on) or the one that stays (off).
        if let Some(entry) = self.tree.entry_mut(id) {
            match entry.state.as_mut() {
                Some(state) => {
                    if let Some(slot) = self.bases.slot(id) {
                        drop(state.commit_filter(&slot.filter));
                    }
                }
                None => entry.pending = Pending::Commit,
            }
            if !on {
                entry.adjusted = false;
            }
            // The filter as it is now is the base: nothing to give back.
            entry.original = None;
            entry.applied = None;
        }
        self.set_tree_prefs(
            TreePrefs {
                auto_adjust: on,
                ..self.prefs.tree
            },
            effects,
        );
    }

    fn tree_row_clicked(&mut self, index: usize, effects: &mut Vec<Effect>) {
        if !self.tree.open || self.tree.searching() || !self.tree.autorefresh {
            return;
        }
        let Some(shown) = &self.tree.shown else {
            return;
        };
        let position = self.session.position();
        if shown.fen != position.fen() {
            return;
        }
        let Some(row) = shown.result.rows.get(index) else {
            return;
        };
        if row.is_end() || row.hidden {
            return;
        }
        let Some(Ok(mv)) = row.uci.as_deref().map(|u| position.parse_uci(u)) else {
            return;
        };
        // The normal flow of a move of the user (M2-1): advance, enter the
        // variation, add, or ask what to do.
        effects.extend(super::update(
            self,
            Message::Game(GameMessage::PlayMove {
                mv,
                options: PlayOptions::default(),
            }),
        ));
    }

    // ---- the refresh ----

    /// Forgets the state of slots whose database changed or went away
    /// (D961, rule 4): the key is the identity of the database and its
    /// number of games.
    fn tree_sync_slots(&mut self) {
        // A closed tree keeps no state worth checking (D988).
        if !self.tree.open {
            return;
        }
        self.tree_check_lock();
        for id in SlotId::all() {
            let Some(entry) = self.tree.entry_mut(id) else {
                continue;
            };
            let same = self
                .bases
                .slot(id)
                .is_some_and(|slot| slot.source == entry.source && slot.total() == entry.len);
            if same {
                continue;
            }
            let epoch = entry.epoch;
            if let Some(cell) = self.tree.slots.get_mut(id.index()) {
                *cell = None;
            }
            if self
                .tree
                .in_flight
                .as_ref()
                .is_some_and(|f| f.slot == id && f.epoch == epoch)
            {
                self.tree.cancel_search();
                self.tree.queued = true;
                self.tree.last_key = None;
            }
        }
    }

    fn tree_key(&self) -> Option<RefreshKey> {
        let id = self.tree_target();
        let slot = self.bases.slot(id)?;
        let prefs = self.prefs.tree;
        let fast = prefs.fast_mode.engine();
        let ply = if fast.is_fast() {
            self.session.game().ply(self.session.cursor()).ok()
        } else {
            None
        };
        Some(RefreshKey {
            slot: id,
            source: slot.source.clone(),
            len: slot.total(),
            fen: self.session.position().fen(),
            ply,
            sort: prefs.sort.engine(),
            fast,
            adjust: prefs.auto_adjust,
            hide_moves: self.tree.training,
            best: self.tree.best.key(),
        })
    }

    /// Starts a search if the tree on screen is not the one for the
    /// position now. Called at the end of every `update`.
    pub(crate) fn refresh_tree(&mut self, effects: &mut Vec<Effect>) {
        self.tree_cache_sync(effects);
        self.refresh_tree_inner(effects);
        self.tree_training_watchdog();
        self.mask_refresh(effects);
    }

    fn refresh_tree_inner(&mut self, effects: &mut Vec<Effect>) {
        self.tree_training_guard();
        self.tree_flush_all_restores();
        self.tree_sync_slots();
        if !self.tree.open || !self.tree.autorefresh {
            return;
        }
        // The annotation run owns the game (D963).
        if self.edits_blocked() {
            return;
        }
        let usable = self
            .bases
            .slot(self.tree_target())
            .is_some_and(|slot| slot.total() > 0 && !slot.needs_reload);
        if !usable {
            if !self.tree.unavailable {
                self.tree.unavailable = true;
                self.tree.cancel_search();
                self.tree.shown = None;
                self.tree.issue = None;
                self.tree.last_key = None;
            }
            return;
        }
        self.tree.unavailable = false;
        // "Adjust" writes the filter of the slot, which a list job also
        // does: wait for the job.
        if self.prefs.tree.auto_adjust && self.flow.list_job.is_some() {
            return;
        }
        let Some(key) = self.tree_key() else {
            return;
        };
        if self.tree.last_key.as_ref() == Some(&key) {
            return;
        }
        self.tree.last_key = Some(key.clone());
        self.tree.cancel_search();
        self.tree.retried = false;
        if self.tree.in_flight.is_some() {
            self.tree.queued = true;
            return;
        }
        let position = self.session.position().clone();
        let ply = self
            .session
            .game()
            .ply(self.session.cursor())
            .unwrap_or_default();
        let (fast, phase) = match self.prefs.tree.fast_mode {
            TreeFastMode::Slow => (FastMode::Slow, Phase::Single),
            TreeFastMode::Fast => (FastMode::Fast, Phase::Single),
            TreeFastMode::FastThenSlow => (FastMode::FastThenSlow, Phase::FastFirst),
        };
        self.tree_start(&key, position, ply, fast, phase, effects);
    }

    fn tree_start(
        &mut self,
        key: &RefreshKey,
        position: Position,
        ply: u32,
        fast: FastMode,
        phase: Phase,
        effects: &mut Vec<Effect>,
    ) {
        let id = key.slot;
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let (source, len, filter) = (slot.source.clone(), slot.total(), slot.filter.clone());
        let cache = self.tree_cache_for(id);
        let epoch = if let Some(entry) = self.tree.entry_mut(id) {
            entry.epoch
        } else {
            let epoch = self.tree.fresh_epoch();
            if let Some(cell) = self.tree.slots.get_mut(id.index()) {
                *cell = Some(SlotTree {
                    source: source.clone(),
                    len,
                    epoch,
                    state: Some(TreeState::default()),
                    adjusted: false,
                    original: None,
                    applied: None,
                    restore: None,
                    pending: Pending::Nothing,
                });
            }
            epoch
        };
        // The user changed the filter since the tree wrote it (a search or
        // filter of the list): it is the new base, as in Scid (D979).
        if self.prefs.tree.auto_adjust
            && let Some(entry) = self.tree.entry_mut(id)
            && entry.applied.as_ref().is_some_and(|a| *a != filter)
        {
            if let Some(state) = entry.state.as_mut() {
                drop(state.commit_filter(&filter));
            }
            entry.original = Some(filter.clone());
            entry.applied = None;
        }
        let state = self
            .tree
            .entry_mut(id)
            .and_then(|entry| entry.state.take())
            .unwrap_or_default();
        let cancel = CancelToken::new();
        let generation = self.tree.generation;
        self.tree.started += 1;
        self.tree.in_flight = Some(InFlight {
            generation,
            slot: id,
            epoch,
            cancel: cancel.clone(),
            phase,
            key: key.clone(),
            position: position.clone(),
            ply,
            filter: filter.clone(),
        });
        effects.push(Effect::TreeSearch {
            job: Box::new(TreeJob {
                generation,
                slot: id,
                epoch,
                source,
                state,
                position,
                ply,
                filter,
                sort: self.prefs.tree.sort.engine(),
                fast,
                adjust: self.prefs.tree.auto_adjust,
                hide_moves: key.hide_moves,
                best: key.best.map(|b| crate::tree::BestSpec {
                    results: b.results,
                    order: b.order,
                    max: b.max,
                    current_ply: self
                        .session
                        .game()
                        .ply(self.session.cursor())
                        .ok()
                        .and_then(|p| u8::try_from(p).ok()),
                }),
                eco: self.eco_book.clone(),
                cancel,
                cache,
            }),
        });
    }

    #[allow(clippy::too_many_lines)] // one flat flow over the answer reads best
    fn tree_finished(&mut self, answer: TreeAnswer, effects: &mut Vec<Effect>) {
        let TreeAnswer {
            generation,
            slot,
            epoch,
            state,
            result,
            best,
            cache_used,
        } = answer;
        let Some(flight) = self
            .tree
            .in_flight
            .take_if(|f| f.generation == generation && f.slot == slot && f.epoch == epoch)
        else {
            return;
        };
        if let Some(cache) = &cache_used
            && matches!(&result, Ok(r) if !r.from_cache)
        {
            self.tree_cache_mark_dirty(slot, cache);
        }
        // 1. The state goes back to its slot, if the slot still has it.
        let mut valid = false;
        if let Some(entry) = self.tree.entry_mut(slot)
            && entry.epoch == epoch
        {
            entry.state = Some(state);
            valid = true;
        }
        if valid {
            if matches!(result, Err(TreeIssue::Internal)) {
                // The state that came back is a blank one: the filter the
                // tree adjusted has no copy in it any more (D986).
                self.tree_reset_adjusted(slot);
            }
            self.tree_settle_pending(slot);
        }
        // 2. A newer search was wanted meanwhile.
        if self.tree.queued {
            self.tree.queued = false;
            self.tree.last_key = None;
            self.refresh_tree(effects);
            return;
        }
        if generation != self.tree.generation || !valid {
            return;
        }
        // 3. The answer is current.
        match result {
            Ok(result) => {
                let mut result = *result;
                let mut stale = false;
                if self.prefs.tree.auto_adjust
                    && let Some(filter) = result.adjusted_filter.take()
                {
                    // The user changed the filter while the search flew: the
                    // adjustment was computed over the old base (D990).
                    stale = self
                        .bases
                        .slot(slot)
                        .is_some_and(|s| s.filter != flight.filter);
                    if stale {
                        self.tree_discard_stale_adjust(slot);
                    } else if !self.tree_apply_adjusted(slot, filter) {
                        // Try again when the list job is over.
                        self.tree.last_key = None;
                    }
                }
                self.tree.issue = None;
                self.tree.retried = false;
                if let Some(rows) = best
                    && self.tree.best.open
                {
                    self.tree.best.set_rows(slot, rows);
                }
                self.tree.shown = Some(Shown {
                    result: Arc::new(result),
                    fen: flight.key.fen.clone(),
                });
                if stale {
                    self.tree.last_key = None;
                    self.refresh_tree(effects);
                } else if flight.phase == Phase::FastFirst {
                    self.tree.generation = self.tree.generation.wrapping_add(1);
                    self.tree_start(
                        &flight.key,
                        flight.position,
                        flight.ply,
                        FastMode::Slow,
                        Phase::Single,
                        effects,
                    );
                }
            }
            Err(TreeIssue::Cancelled) => self.tree.last_key = None,
            Err(TreeIssue::FilterSize) if !self.tree.retried => {
                // A state that does not fit the database: start over once.
                self.tree.retried = true;
                self.tree_reset_adjusted(slot);
                if let Some(state) = self
                    .tree
                    .entry_mut(slot)
                    .and_then(|entry| entry.state.as_mut())
                {
                    state.invalidate();
                }
                self.tree.last_key = None;
            }
            Err(issue) => {
                self.tree.shown = None;
                self.tree.issue = Some(issue);
                self.tree.best.clear();
                self.tree.training_wait = None;
            }
        }
        self.tree_try_answer(effects);
    }

    /// Drops an adjustment computed over a filter the user has since
    /// replaced: the user's filter becomes the base of the engine state
    /// (D990).
    fn tree_discard_stale_adjust(&mut self, id: SlotId) {
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        if let Some(entry) = self.tree.entry_mut(id)
            && entry.applied.is_none()
            && let Some(state) = entry.state.as_mut()
        {
            drop(state.commit_filter(&slot.filter));
        }
    }

    /// What was deferred while the state of `slot` was out.
    fn tree_settle_pending(&mut self, slot: SlotId) {
        let Some(entry) = self.tree.entry_mut(slot) else {
            return;
        };
        match std::mem::replace(&mut entry.pending, Pending::Nothing) {
            Pending::Nothing => {}
            Pending::Clean => self.tree_end_entry(slot),
            Pending::Commit => {
                if let (Some(state), Some(base)) = (entry.state.as_mut(), self.bases.slot(slot)) {
                    drop(state.commit_filter(&base.filter));
                }
            }
        }
    }

    /// Makes `filter` the main filter of the slot and puts the list at its
    /// end (D967). Returns whether it did.
    fn tree_apply_adjusted(&mut self, id: SlotId, filter: Filter) -> bool {
        if self.flow.list_job.is_some() {
            return false;
        }
        let Some(slot) = self.bases.slot_mut(id) else {
            return false;
        };
        if filter.len() != slot.filter.len() {
            return false;
        }
        let before = std::mem::replace(&mut slot.filter, filter);
        slot.filter_changed();
        slot.list.first = slot.filter.count().max(1);
        let applied = slot.filter.clone();
        if let Some(entry) = self.tree.entry_mut(id) {
            if !entry.adjusted || entry.original.is_none() {
                entry.original = Some(before);
            }
            entry.adjusted = true;
            entry.applied = Some(applied);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    //! Sanity tests of the tree flow. The full contract suite is the
    //! tester's.

    use std::collections::VecDeque;

    use super::*;
    use crate::bases::BasesMessage;
    use crate::effect::perform;
    use crate::state::update;

    const GAMES: &str = "[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
        [Result \"0-1\"]\n\n1. d4 d5 0-1\n\n\
        [Result \"1/2-1/2\"]\n\n1. e4 c5 1/2-1/2\n\n\
        [Result \"1-0\"]\n\n1. Nf3 d5 1-0\n\n\
        [Result \"*\"]\n\n1. e4 *\n\n";

    async fn drive(state: &mut AppState, message: impl Into<Message>) {
        let mut queue: VecDeque<Effect> = update(state, message.into()).into();
        while let Some(effect) = queue.pop_front() {
            if effect.runs_on_host() {
                continue;
            }
            let answer = perform(effect).await;
            queue.extend(update(state, answer));
        }
    }

    #[allow(clippy::field_reassign_with_default)] // private fields forbid the struct update
    async fn opened() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("g.pgn");
        std::fs::write(&path, GAMES).expect("write");
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        drive(&mut state, BasesMessage::Open(path)).await;
        assert_eq!(
            state
                .bases
                .current_slot()
                .map(crate::bases::BaseSlot::total),
            Some(5)
        );
        (state, dir)
    }

    fn open_tree() -> Message {
        Message::Tree(TreeMessage::Open)
    }

    #[tokio::test]
    async fn opening_shows_the_tree_of_the_start_position() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        let view = state.tree_view();
        assert_eq!(view.body, TreeBody::Rows);
        assert!(!view.searching);
        let sans: Vec<&str> = view.rows.iter().map(|r| r.san.as_str()).collect();
        assert_eq!(sans.first(), Some(&"e4"));
        assert_eq!(sans.len(), 3);
        assert_eq!(view.rows[0].games, 3);
        assert_eq!(view.total.as_ref().map(|t| t.games), Some(5));
        assert!(view.rows.iter().all(|r| r.clickable));
        assert!(!view.rows.iter().any(|r| r.is_next_move));
    }

    #[tokio::test]
    async fn nothing_changes_nothing_starts() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        let started = state.tree().searches_started();
        let effects = update(&mut state, Message::Tree(TreeMessage::SetShort(false)));
        assert!(effects.is_empty());
        assert_eq!(state.tree().searches_started(), started);
        assert!(!state.tree_view().short);
    }

    #[tokio::test]
    async fn a_click_plays_the_move_and_the_tree_follows() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        drive(&mut state, Message::Tree(TreeMessage::RowClicked(0))).await;
        assert_eq!(state.session.game().ply(state.session.cursor()), Ok(1));
        let view = state.tree_view();
        assert_eq!(view.body, TreeBody::Rows);
        let mut sans: Vec<&str> = view.rows.iter().map(|r| r.san.as_str()).collect();
        sans.sort_unstable();
        assert_eq!(sans, ["[end]", "c5", "e5"]);
        assert!(view.rows.iter().any(|r| r.is_end && !r.clickable));
    }

    #[tokio::test]
    async fn the_next_move_of_the_game_is_marked() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        drive(&mut state, Message::Tree(TreeMessage::RowClicked(0))).await;
        drive(
            &mut state,
            Message::Game(GameMessage::Navigate(crate::session::Nav::Start)),
        )
        .await;
        let view = state.tree_view();
        let next: Vec<&str> = view
            .rows
            .iter()
            .filter(|r| r.is_next_move)
            .map(|r| r.san.as_str())
            .collect();
        assert_eq!(next, ["e4"]);
    }

    #[tokio::test]
    async fn a_late_answer_is_dropped_and_the_newest_search_wins() {
        let (mut state, _dir) = Box::pin(opened()).await;
        let first = update(&mut state, open_tree());
        assert_eq!(first.len(), 1);
        let generation = state.tree().generation();
        // The position changes while the first search is out.
        let mut game = mychess_core::Game::new();
        let mut pos = Position::new();
        let mv = pos.parse_uci("d2d4").expect("legal");
        pos.play(mv).expect("play");
        game.add_move(game.root(), mv).expect("add");
        state.session.replace_game(game);
        let second = update(
            &mut state,
            Message::Game(GameMessage::Navigate(crate::session::Nav::End)),
        );
        assert!(second.is_empty(), "waits for the search in flight");
        assert!(state.tree().generation() > generation);
        assert!(state.tree().searching());
        // The first search returns: its result is not shown, the next starts.
        let mut queue: VecDeque<Effect> = first.into();
        while let Some(effect) = queue.pop_front() {
            let answer = perform(effect).await;
            queue.extend(update(&mut state, answer));
        }
        let view = state.tree_view();
        assert_eq!(view.body, TreeBody::Rows);
        let sans: Vec<&str> = view.rows.iter().map(|r| r.san.as_str()).collect();
        assert_eq!(sans, ["d5"]);
        assert_eq!(state.tree().searches_started(), 2);
    }

    #[tokio::test]
    async fn adjust_filters_the_list_and_closing_restores_it() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        drive(&mut state, Message::Tree(TreeMessage::RowClicked(0))).await;
        drive(&mut state, Message::Tree(TreeMessage::SetAdjust(true))).await;
        assert_eq!(
            state
                .bases
                .current_slot()
                .map(crate::bases::BaseSlot::filtered),
            Some(3)
        );
        drive(&mut state, Message::Tree(TreeMessage::Close)).await;
        assert_eq!(
            state
                .bases
                .current_slot()
                .map(crate::bases::BaseSlot::filtered),
            Some(5)
        );
        assert_eq!(state.tree_view().body, TreeBody::Closed);
    }

    fn filtered(state: &AppState) -> Option<u32> {
        state
            .bases
            .current_slot()
            .map(crate::bases::BaseSlot::filtered)
    }

    #[tokio::test]
    async fn a_filter_the_user_makes_while_adjusting_becomes_the_base() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        drive(&mut state, Message::Tree(TreeMessage::SetAdjust(true))).await;
        // The user filters the list down to game 1 (as a list search would).
        let id = state.bases.current();
        if let Some(slot) = state.bases.slot_mut(id) {
            slot.filter.fill(0);
            slot.filter.set(1, 1);
        }
        drive(&mut state, Message::Tree(TreeMessage::RowClicked(0))).await;
        assert_eq!(
            filtered(&state),
            Some(1),
            "the tree narrows the user filter"
        );
        drive(&mut state, Message::Tree(TreeMessage::Close)).await;
        assert_eq!(filtered(&state), Some(1), "the user filter is not lost");
    }

    #[tokio::test]
    async fn a_failure_gives_the_adjusted_filter_back() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        drive(&mut state, Message::Tree(TreeMessage::RowClicked(0))).await;
        drive(&mut state, Message::Tree(TreeMessage::SetAdjust(true))).await;
        assert_eq!(filtered(&state), Some(3));
        let id = state.bases.current();
        state.tree_reset_adjusted(id);
        assert_eq!(filtered(&state), Some(5));
    }

    #[tokio::test]
    async fn turning_adjust_off_keeps_the_adjusted_filter() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        drive(&mut state, Message::Tree(TreeMessage::RowClicked(0))).await;
        drive(&mut state, Message::Tree(TreeMessage::SetAdjust(true))).await;
        drive(&mut state, Message::Tree(TreeMessage::SetAdjust(false))).await;
        drive(&mut state, Message::Tree(TreeMessage::Close)).await;
        assert_eq!(
            state
                .bases
                .current_slot()
                .map(crate::bases::BaseSlot::filtered),
            Some(3)
        );
    }

    #[tokio::test]
    async fn an_empty_base_is_unavailable_and_searches_nothing() {
        let mut state = AppState::default();
        let effects = update(&mut state, open_tree());
        assert!(effects.is_empty());
        assert_eq!(state.tree_view().body, TreeBody::Unavailable);
    }

    #[tokio::test]
    async fn fast_then_slow_runs_two_searches() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(
            &mut state,
            Message::Tree(TreeMessage::SetFastMode(TreeFastMode::FastThenSlow)),
        )
        .await;
        drive(&mut state, open_tree()).await;
        assert_eq!(state.tree().searches_started(), 2);
        assert_eq!(state.tree_view().body, TreeBody::Rows);
    }

    #[tokio::test]
    async fn closing_the_base_forgets_its_state() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, open_tree()).await;
        let current = state.bases.current();
        drive(&mut state, BasesMessage::Close(current)).await;
        assert!(state.tree.slots.iter().all(Option::is_none));
    }
}
