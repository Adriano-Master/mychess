//! The cache of the opening trees in the state (spec M5-3b, D1760-D1789).
//!
//! Each slot has at most one [`CacheEntry`]: the [`StcCache`] of its
//! database behind a mutex. The searches of the tree use it (D1765), the
//! menu of the panel saves, fills and describes it with one job at a time
//! (D1766), and **one point** keeps it honest: [`AppState::tree_cache_sync`],
//! called at the end of every `update`, drops the cache and removes the
//! `.stc` file of a database whose content changed (D1767).

use mychess_db::Filter;
use mychess_db::tree::StcCache;

use super::AppState;
use crate::bases::{BaseKind, OpenControl, SlotId};
use crate::effect::Effect;
use crate::tree::{
    CACHE_SIZE_RANGE, CacheAnswer, CacheBusy, CacheDiscard, CacheEntry, CacheFill, CacheIssue,
    CacheJob, CacheJobKind, CacheOutcome, CacheStatus, CacheView, RunningJob, SharedStc,
    TreeMessage, TreePrefs,
};

/// What happened to the database of a cache.
enum Verdict {
    /// Same database, same content.
    Same,
    /// Another database (or none) is in the slot now: the cache goes, the
    /// file stays.
    Replaced,
    /// The same database with other content: the cache and the file go.
    Changed,
}

impl AppState {
    /// Whether a job of the cache of the trees runs (the front-end redraws
    /// its progress while it does).
    pub fn tree_cache_busy(&self) -> bool {
        self.tree.cache.job.is_some()
    }

    /// The cache part of the panel (D1768).
    pub(crate) fn tree_cache_view(&self) -> CacheView {
        let ui = &self.tree.cache;
        let busy = ui.job.as_ref().map(|job| {
            let (done, total) = job.control.progress();
            CacheBusy {
                kind: job.kind,
                done,
                total,
            }
        });
        let has_file = self
            .bases
            .slot(self.tree_target())
            .is_some_and(|slot| slot.kind == BaseKind::Si4 && slot.path.is_some());
        CacheView {
            size: self.prefs.tree.cache_size,
            autosave: self.prefs.tree.cache_autosave,
            busy,
            status: ui.status.clone(),
            has_file,
        }
    }

    /// The cache jobs and the options of the menu.
    pub(crate) fn tree_cache_message(&mut self, message: TreeMessage, effects: &mut Vec<Effect>) {
        match message {
            TreeMessage::SaveCache => self.tree_cache_start(CacheJobKind::Save, effects),
            TreeMessage::FillCache(fill) => {
                self.tree_cache_start(CacheJobKind::Fill(fill), effects);
            }
            TreeMessage::CacheInfo => self.tree_cache_start(CacheJobKind::Info, effects),
            TreeMessage::CancelCacheJob => {
                if let Some(job) = &self.tree.cache.job {
                    job.control.cancel();
                }
            }
            TreeMessage::DismissCacheStatus => self.tree.cache.status = CacheStatus::None,
            TreeMessage::SetCacheSize(size) => {
                let size = size.clamp(*CACHE_SIZE_RANGE.start(), *CACHE_SIZE_RANGE.end());
                self.tree_cache_prefs(
                    TreePrefs {
                        cache_size: size,
                        ..self.prefs.tree
                    },
                    effects,
                );
            }
            TreeMessage::SetCacheAutosave(on) => self.tree_cache_prefs(
                TreePrefs {
                    cache_autosave: on,
                    ..self.prefs.tree
                },
                effects,
            ),
            TreeMessage::CacheFinished(answer) => self.tree_cache_finished(*answer, effects),
            TreeMessage::CacheDiscarded(file) => {
                if let Some(at) = self.tree.cache.discarding.iter().position(|f| *f == file) {
                    self.tree.cache.discarding.remove(at);
                }
                self.tree_cache_quit_check(effects);
            }
            TreeMessage::CacheDeadline(generation)
                if self.tree.cache.deadline_armed
                    && generation == self.tree.cache.deadline_generation
                    && !self.tree.cache.quit_finished =>
            {
                // A timer of an older wait (or of none) is ignored (D1780).
                self.tree.cache.autosaves = 0;
                self.tree.cache.discarding.clear();
                self.tree_cache_quit_check(effects);
            }
            _ => {}
        }
    }

    fn tree_cache_prefs(&mut self, prefs: TreePrefs, effects: &mut Vec<Effect>) {
        if prefs != self.prefs.tree {
            self.prefs_message(
                crate::prefs::PrefsMessage::Set(crate::prefs::PrefChange::Tree(prefs)),
                effects,
            );
        }
    }

    /// The cache of the slot, created if the slot has none (D1765). `None`
    /// when the slot has no database.
    pub(super) fn tree_cache_for(&mut self, id: SlotId) -> Option<SharedStc> {
        let slot = self.bases.slot(id)?;
        if let Some(entry) = self.tree.cache.entry(id)
            && entry.source == slot.source
            && entry.len == slot.total()
        {
            return Some(entry.cache.clone());
        }
        // Only a database in `.si4` files has a cache file (D1049); the
        // others keep the cache in memory.
        let file = (slot.kind == BaseKind::Si4)
            .then(|| slot.path.clone())
            .flatten();
        // The file of the old content is still being removed: a cache made
        // now could read it (D1772). The search runs without a cache.
        if let Some(file) = &file
            && self
                .tree
                .cache
                .discarding
                .iter()
                .any(|f| crate::tree::same_stc(f, file))
        {
            return None;
        }
        let mut cache = StcCache::new(file.as_deref());
        let size = self.prefs.tree.cache_size;
        cache.resize(size);
        let entry = CacheEntry {
            source: slot.source.clone(),
            len: slot.total(),
            identity: slot.path.clone(),
            file,
            cache: std::sync::Arc::new(std::sync::Mutex::new(cache)),
            dirty: false,
            size,
        };
        let shared = entry.cache.clone();
        if let Some(cell) = self.tree.cache.entries.get_mut(id.index()) {
            *cell = Some(entry);
        }
        Some(shared)
    }

    /// Marks the cache of `id` as holding trees that are not on disk; does
    /// nothing if `cache` is not the one the slot has now (D1773).
    pub(super) fn tree_cache_mark_dirty(&mut self, id: SlotId, cache: &SharedStc) {
        if let Some(entry) = self.tree.cache.entry_mut(id)
            && std::sync::Arc::ptr_eq(&entry.cache, cache)
        {
            entry.dirty = true;
        }
    }

    /// The database of the slot was opened again after a change that leaves
    /// the trees as they were (the header, a flag that is not `D`: spec M7-1,
    /// D3534): the cache is bound to the new database instead of dropped.
    pub(super) fn tree_cache_rebind(&mut self, id: SlotId) {
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let (source, total) = (slot.source.clone(), slot.total());
        if let Some(entry) = self.tree.cache.entry_mut(id)
            && entry.len == total
        {
            entry.source = source;
        }
    }

    /// Forgets the cache of a slot without touching its file: the file was
    /// just written by the Cleaner and the next search reads it (spec M7-1,
    /// rule 47).
    pub(super) fn tree_cache_forget(&mut self, id: SlotId) {
        if let Some(cell) = self.tree.cache.entries.get_mut(id.index()) {
            *cell = None;
        }
    }

    // ---- the invalidation point (D1767) ----

    /// Drops the cache of every slot whose database changed or went away,
    /// removes the `.stc` file of the ones whose content changed, and
    /// applies the configured size. Called at the end of every `update`:
    /// every path that writes a database reopens it (a new
    /// [`SharedSource`](crate::bases::SharedSource)) or changes its number
    /// of games, so none can forget to invalidate.
    pub(super) fn tree_cache_sync(&mut self, effects: &mut Vec<Effect>) {
        let size = self.prefs.tree.cache_size;
        for id in SlotId::all() {
            let Some(entry) = self.tree.cache.entry(id) else {
                continue;
            };
            let size_differs = entry.size != size;
            let verdict = match self.bases.slot(id) {
                None => Verdict::Replaced,
                Some(slot) if slot.path.as_deref() != entry.identity.as_deref() => {
                    Verdict::Replaced
                }
                Some(slot)
                    if slot.needs_reload
                        || slot.source != entry.source
                        || slot.total() != entry.len =>
                {
                    Verdict::Changed
                }
                Some(_) => Verdict::Same,
            };
            match verdict {
                Verdict::Same => {
                    if size_differs
                        && let Some(entry) = self.tree.cache.entry_mut(id)
                        && let Some(mut guard) = crate::tree::try_lock(&entry.cache)
                    {
                        guard.resize(size);
                        entry.size = size;
                    }
                }
                Verdict::Replaced => self.tree_cache_drop(id, false, effects),
                Verdict::Changed => self.tree_cache_drop(id, true, effects),
            }
        }
    }

    /// Invalidates the cache of a slot: the entry goes, and with `remove`
    /// the cache is emptied (`StcCache::clear`) and its `.stc` file removed
    /// (`remove_stc`, D1050) by an effect that waits for the lock of the
    /// cache, so a save in flight cannot write the old trees after the
    /// removal (D1772). The `stale` flag stops a save that has not written
    /// yet.
    fn tree_cache_drop(&mut self, id: SlotId, remove: bool, effects: &mut Vec<Effect>) {
        let Some(cell) = self.tree.cache.entries.get_mut(id.index()) else {
            return;
        };
        let Some(entry) = cell.take() else {
            return;
        };
        if let Some(job) = &self.tree.cache.job
            && job.slot == id
        {
            job.control.cancel();
        }
        if remove {
            if let Some(file) = entry.file {
                // Every save of this file made until now must not write,
                // whatever cache it belongs to (D1779).
                crate::tree::file_guard(&file).invalidate();
                self.tree.cache.discarding.push(file.clone());
                effects.push(Effect::TreeCacheDiscard {
                    discard: Box::new(CacheDiscard {
                        cache: entry.cache,
                        file,
                    }),
                });
            }
        } else if self.prefs.tree.cache_autosave && entry.dirty {
            self.tree_cache_autosave_entry(id, &entry, effects);
        }
    }

    // ---- saving without being asked (D1761) ----

    fn tree_cache_autosave_entry(
        &mut self,
        id: SlotId,
        entry: &CacheEntry,
        effects: &mut Vec<Effect>,
    ) {
        if entry.file.is_none() {
            return;
        }
        let job = self.tree_cache_job(
            id,
            CacheJobKind::Save,
            true,
            entry.cache.clone(),
            entry.source.clone(),
            Filter::new(0),
            None,
            OpenControl::new(),
            entry.file.clone(),
        );
        self.tree.cache.autosaves += 1;
        effects.push(Effect::TreeCache { job: Box::new(job) });
    }

    /// Saves every cache that holds trees not on disk; with "auto-save" on
    /// only (D1761). Used when the panel closes.
    pub(super) fn tree_cache_autosave_all(&mut self, effects: &mut Vec<Effect>) {
        if !self.prefs.tree.cache_autosave {
            return;
        }
        for id in SlotId::all() {
            let Some(entry) = self.tree.cache.entry_mut(id) else {
                continue;
            };
            if !entry.dirty || entry.file.is_none() {
                continue;
            }
            entry.dirty = false;
            let (cache, source, file) = (
                entry.cache.clone(),
                entry.source.clone(),
                entry.file.clone(),
            );
            let job = self.tree_cache_job(
                id,
                CacheJobKind::Save,
                true,
                cache,
                source,
                Filter::new(0),
                None,
                OpenControl::new(),
                file,
            );
            self.tree.cache.autosaves += 1;
            effects.push(Effect::TreeCache { job: Box::new(job) });
        }
    }

    /// Called by the quit flow before the program exits (D1769): stops the
    /// fill that runs, saves the caches if "auto-save" is on and says
    /// whether the quit must wait for the saves and for the discards of
    /// stale files (D1781).
    pub(crate) fn tree_cache_quit_wait(&mut self, effects: &mut Vec<Effect>) -> bool {
        if !self.tree.cache.quit_started {
            self.tree.cache.quit_started = true;
            if let Some(job) = &self.tree.cache.job {
                job.control.cancel();
            }
            self.tree_cache_autosave_all(effects);
            if self.tree_cache_pending() {
                // The saves wait for a fill that was cancelled, or write a
                // big file: do not wait for ever (D1776). The timer has an
                // identity, so no other timer can end this wait (D1780).
                self.tree.cache.deadline_armed = true;
                self.tree.cache.deadline_generation += 1;
                effects.push(Effect::TreeCacheDeadline {
                    delay: std::time::Duration::from_secs(5),
                    generation: self.tree.cache.deadline_generation,
                });
            }
        }
        !self.tree.cache.quit_finished && self.tree_cache_pending()
    }

    fn tree_cache_pending(&self) -> bool {
        self.tree.cache.autosaves > 0 || !self.tree.cache.discarding.is_empty()
    }

    /// Continues the quit when nothing of the cache is pending; once only
    /// (D1782).
    fn tree_cache_quit_check(&mut self, effects: &mut Vec<Effect>) {
        if self.tree.cache.quit_started
            && !self.tree.cache.quit_finished
            && !self.tree_cache_pending()
        {
            self.tree.cache.quit_finished = true;
            self.tree.cache.deadline_armed = false;
            self.finish_quit(effects);
        }
    }

    // ---- the jobs of the menu (D1766) ----

    #[allow(clippy::too_many_arguments)] // plain data of one job
    fn tree_cache_job(
        &mut self,
        slot: SlotId,
        kind: CacheJobKind,
        autosave: bool,
        cache: SharedStc,
        source: crate::bases::SharedSource,
        filter: Filter,
        game: Option<Box<mychess_core::Game>>,
        control: OpenControl,
        file: Option<std::path::PathBuf>,
    ) -> CacheJob {
        self.tree.cache.next_job += 1;
        CacheJob {
            id: self.tree.cache.next_job,
            slot,
            kind,
            autosave,
            cache,
            epoch: file.as_deref().map(crate::tree::FileEpoch::of),
            path: file,
            source,
            filter,
            game,
            eco: self.eco_book.clone(),
            control,
        }
    }

    fn tree_cache_start(&mut self, kind: CacheJobKind, effects: &mut Vec<Effect>) {
        let fail =
            |state: &mut AppState, issue| state.tree.cache.status = CacheStatus::Failed(issue);
        if self.tree.cache.job.is_some() {
            fail(self, CacheIssue::Busy);
            return;
        }
        let id = self.tree_target();
        let usable = self
            .bases
            .slot(id)
            .is_some_and(|slot| slot.total() > 0 && !slot.needs_reload);
        if !usable {
            fail(self, CacheIssue::NoBase);
            return;
        }
        let has_file = self
            .bases
            .slot(id)
            .is_some_and(|slot| slot.kind == BaseKind::Si4);
        if !has_file && !matches!(kind, CacheJobKind::Info) {
            fail(self, CacheIssue::NotScidBase);
            return;
        }
        let Some(cache) = self.tree_cache_for(id) else {
            // The slot has a database: the cache is not made while the file
            // of the old content is being removed (D1783).
            fail(self, CacheIssue::Discarding);
            return;
        };
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let source = slot.source.clone();
        let filter = match kind {
            CacheJobKind::Fill(CacheFill::Base) => slot.filter.clone(),
            _ => Filter::new(0),
        };
        let game = match kind {
            CacheJobKind::Fill(CacheFill::Game) => Some(Box::new(self.session.game().clone())),
            _ => None,
        };
        let control = OpenControl::new();
        let file = self
            .tree
            .cache
            .entry(id)
            .and_then(|entry| entry.file.clone());
        let job = self.tree_cache_job(
            id,
            kind,
            false,
            cache,
            source,
            filter,
            game,
            control.clone(),
            file,
        );
        self.tree.cache.status = CacheStatus::None;
        self.tree.cache.job = Some(RunningJob {
            id: job.id,
            slot: id,
            kind,
            control,
        });
        // A fill dirties the cache from its start, so a quit or a close
        // while it runs saves what it did (D1774).
        if matches!(kind, CacheJobKind::Fill(_))
            && let Some(entry) = self.tree.cache.entry_mut(id)
        {
            entry.dirty = true;
        }
        effects.push(Effect::TreeCache { job: Box::new(job) });
    }

    fn tree_cache_finished(&mut self, answer: CacheAnswer, effects: &mut Vec<Effect>) {
        let CacheAnswer {
            id,
            slot,
            autosave,
            cache,
            result,
        } = answer;
        // Whether the cache the job worked on is still the one of the slot
        // (D1773): if not, its answer says nothing about the slot.
        let current = self
            .tree
            .cache
            .entry(slot)
            .is_some_and(|entry| std::sync::Arc::ptr_eq(&entry.cache, &cache));
        if autosave {
            self.tree.cache.autosaves = self.tree.cache.autosaves.saturating_sub(1);
            if let Err(issue) = &result {
                tracing::warn!(?issue, "the tree cache could not be saved");
                // The trees are still not on disk.
                self.tree_cache_mark_dirty(slot, &cache);
            }
            self.tree_cache_quit_check(effects);
            return;
        }
        let Some(job) = self.tree.cache.job.take_if(|job| job.id == id) else {
            return;
        };
        self.tree.cache.status = match result {
            // The database changed while the job ran: what it did is not
            // worth reporting.
            Ok(CacheOutcome::Saved { .. } | CacheOutcome::Filled(_)) if !current => {
                CacheStatus::Failed(CacheIssue::Stale)
            }
            Ok(CacheOutcome::Saved { bytes, skipped }) => {
                // Only a save that worked puts the cache on disk (D1775).
                if current && let Some(entry) = self.tree.cache.entry_mut(slot) {
                    entry.dirty = false;
                }
                CacheStatus::Saved { bytes, skipped }
            }
            Ok(CacheOutcome::Filled(report)) => {
                self.tree_cache_mark_dirty(slot, &cache);
                CacheStatus::Filled {
                    searches: report.searches,
                    hits: report.cache_hits,
                }
            }
            Ok(CacheOutcome::Info(info)) => CacheStatus::Info(info),
            Err(_) if !current => CacheStatus::Failed(CacheIssue::Stale),
            Err(issue) => {
                // What a cancelled fill did stays in the cache.
                if matches!(job.kind, CacheJobKind::Fill(_)) {
                    self.tree_cache_mark_dirty(slot, &cache);
                }
                CacheStatus::Failed(issue)
            }
        };
    }
}

#[cfg(test)]
mod tests {
    //! Sanity tests of the cache flow. The contract suite is in
    //! `tests/m5_3b_*.rs`.

    use std::collections::VecDeque;

    use super::*;
    use crate::bases::BasesMessage;
    use crate::effect::perform;
    use crate::message::Message;
    use crate::state::update;
    use crate::tree::TreeBody;

    const GAMES: &str = "[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
        [Result \"0-1\"]\n\n1. d4 d5 0-1\n\n";

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
        (state, dir)
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // the point: the lock is held while the search runs
    async fn a_search_does_not_wait_for_a_cache_that_is_held() {
        let (mut state, _dir) = Box::pin(opened()).await;
        let id = state.bases.current();
        let cache = state.tree_cache_for(id).expect("cache");
        let held = crate::tree::try_lock(&cache).expect("free");
        // The tree opens while a job holds the cache: it still answers.
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        assert_eq!(state.tree_view().body, TreeBody::Rows);
        assert!(!state.tree.cache.entry(id).expect("entry").dirty);
        drop(held);
    }

    #[tokio::test]
    async fn a_slow_search_marks_the_cache_dirty_and_saving_clears_it() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        let id = state.bases.current();
        assert!(state.tree.cache.entry(id).expect("entry").dirty);
        // A PGN file has no cache file: the save is refused, still dirty.
        drive(&mut state, Message::Tree(TreeMessage::SaveCache)).await;
        assert_eq!(
            state.tree.cache.status,
            CacheStatus::Failed(CacheIssue::NotScidBase)
        );
        assert!(state.tree.cache.entry(id).expect("entry").dirty);
    }

    #[tokio::test]
    async fn closing_the_base_drops_the_cache_without_touching_files() {
        let (mut state, _dir) = Box::pin(opened()).await;
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        let id = state.bases.current();
        assert!(state.tree.cache.entry(id).is_some());
        drive(&mut state, BasesMessage::Close(id)).await;
        assert!(state.tree.cache.entry(id).is_none());
    }
}
