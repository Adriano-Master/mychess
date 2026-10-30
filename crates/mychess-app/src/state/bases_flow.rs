//! The database slots and the Clipbase (spec M3-4, slice 4a; D500 to D507
//! and D513 onwards).
//!
//! Everything here is pure bookkeeping: opening a file and reading a game of
//! a file are [`Effect`]s, and their answers come back as
//! [`BasesMessage::Opened`] and [`BasesMessage::GameLoaded`]. The one thing
//! done inside `update` is the Clipbase, which lives in memory: copying
//! encodes one game and appends it incrementally, pasting decodes one (D516,
//! D524). The cost of a copy does not depend on the size of the Clipbase.

use std::path::PathBuf;

use mychess_core::Game;

use super::{AppState, PendingBaseGame, PendingOpen};
use crate::bases::{
    BaseError, BaseKind, BaseSlot, Bases, BasesMessage, BasesNotice, Clipbase, ListJobKind,
    OpenControl, OpenOutcome, Parked, SlotId,
};
use crate::effect::Effect;
use crate::error::{FileErrorKind, RequestId};
use crate::files::GameOrigin;
use crate::flow::{AppNotice, BrowserLanding, DiscardPrompt, Pending, Replacement};
use crate::session::{GameMessage, Nav};
use crate::shared::SharedGame;
use mychess_db::GameSource;

impl AppState {
    /// The control of the database being opened, if one is: the front-end
    /// reads its progress and may cancel it (see [`OpenControl`]).
    pub fn base_opening(&self) -> Option<&OpenControl> {
        self.flow.open_base.as_ref().map(|open| &open.control)
    }

    /// The file being opened, if one is (for the progress shown by the
    /// front-end).
    pub fn base_opening_path(&self) -> Option<&std::path::Path> {
        self.flow.open_base.as_ref().map(|open| open.path.as_path())
    }

    /// Handles a [`BasesMessage`].
    pub(crate) fn bases_message(&mut self, message: BasesMessage, effects: &mut Vec<Effect>) {
        match message {
            // Answers to effects: always handled, even with a dialog open.
            BasesMessage::Opened { request, result } => {
                // Only the answer for the file the tree asked for counts
                // (D1741): a request that was cancelled or failed must not
                // lock the tree on a later, unrelated open.
                let for_tree = self.flow.open_base.as_ref().is_some_and(|o| {
                    o.request == request
                        && self.tree.open_as_tree.as_deref() == Some(o.path.as_path())
                });
                let before = self.bases.current();
                self.base_opened(request, result, effects);
                if for_tree {
                    self.tree_after_open(before);
                }
                // The next file named on the command line (spec M9-4).
                self.start_open_next(effects);
            }
            BasesMessage::GameLoaded { request, result } => {
                self.base_game_loaded(request, result, effects);
            }
            BasesMessage::QuitDeadline => {
                if self.flow.quit_waiting_batch {
                    // The batch step did not answer in time: leave without it.
                    self.flow.quit_waiting_batch = false;
                    self.flow.annotate_batch = None;
                    self.finish_quit(effects);
                    return;
                }
                if self.flow.quit_waiting_job {
                    // The job did not answer in time: leave without it.
                    self.flow.quit_waiting_job = false;
                    self.flow.list_job = None;
                    self.finish_quit(effects);
                    return;
                }
                if self.tactics_quit_deadline() {
                    // The write of the tactics score did not answer in time.
                    self.finish_quit(effects);
                }
            }
            BasesMessage::CancelOpen => {
                if let Some(open) = &self.flow.open_base {
                    open.control.cancel();
                }
            }
            // The actions of the list decide for themselves what to do
            // while a dialog is open (their answers must always get in).
            BasesMessage::List(message) => self.list_message(message, effects),
            // The browser decides for itself what to do while a dialog is
            // open (its answers must always get in).
            BasesMessage::Browse(message) => self.browse_message(message, effects),
            // Requests from the user: ignored while a dialog that edits or
            // replaces the game is open, like the other commands.
            _ if self.modal_open() => {}
            BasesMessage::Open(path) => self.start_open_base(path, effects),
            BasesMessage::Select(slot) => self.select_slot(slot),
            BasesMessage::Next => self.select_next(true),
            BasesMessage::Previous => self.select_next(false),
            BasesMessage::Close(slot) => self.close_request(slot, effects),
            BasesMessage::LoadGame { slot, number } => self.load_base_game(slot, number, effects),
            BasesMessage::ClipbaseCopy => self.clipbase_copy(),
            BasesMessage::ClipbasePaste => self.clipbase_paste(effects),
            BasesMessage::ClipbaseClear => self.clipbase_clear(),
            BasesMessage::SetListFirst { slot, first } => {
                if let Some(slot) = self.bases.slot_mut(slot) {
                    slot.list.first = first;
                    slot.clamp_first();
                }
            }
            BasesMessage::SelectRow { slot, number } => {
                if let Some(slot) = self.bases.slot_mut(slot)
                    && number.is_none_or(|n| slot.filter.contains(n))
                {
                    slot.list.select_only(number);
                }
            }
            BasesMessage::SetSearchText { slot, text } => {
                if let Some(slot) = self.bases.slot_mut(slot) {
                    slot.list.search = text;
                }
            }
        }
    }

    // ---- opening ----

    /// Opens `path` in the first free slot, or activates the slot that
    /// holds it. The file is read by an effect.
    pub(crate) fn start_open_base(&mut self, path: PathBuf, effects: &mut Vec<Effect>) {
        if self.refuse_edit() {
            return;
        }
        if self.flow.open_base.is_some() {
            self.notice = Some(AppNotice::Bases(BasesNotice::OpenInProgress));
            return;
        }
        // A closed target may still be written by a job winding down.
        if self
            .flow
            .list_job
            .as_ref()
            .is_some_and(|j| j.closing && j.target_path.as_deref() == Some(path.as_path()))
        {
            self.notice = Some(AppNotice::Bases(BasesNotice::ListBusy));
            return;
        }
        // The path as given may already be the canonical one of a slot (a
        // recent file): no need to ask the file system.
        if let Some(slot) = self.bases.find_by_path(&path) {
            self.select_slot(slot);
            return;
        }
        let request = self.new_request();
        let control = OpenControl::new();
        self.flow.open_base = Some(PendingOpen {
            request,
            path: path.clone(),
            control: control.clone(),
        });
        effects.push(Effect::OpenBase {
            request,
            path,
            open_paths: self.bases.open_paths(),
            allow_new: self.bases.first_free().is_some(),
            control,
        });
    }

    fn base_opened(
        &mut self,
        request: RequestId,
        result: Result<OpenOutcome, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.open_base.as_ref().map(|o| o.request) != Some(request) {
            return;
        }
        let Some(pending) = self.flow.open_base.take() else {
            return;
        };
        let bookmark = self.take_bookmark_go(&pending.path);
        match result {
            Err(BaseError::Cancelled) => {}
            Err(_) if bookmark.is_some() => {
                // A bookmark whose database is gone stays in the list; the
                // remark names it (D680). A bookmark never changes the recent
                // files (D706).
                let title = bookmark.map(|b| b.title).unwrap_or_default();
                self.notice = Some(AppNotice::Bookmark(
                    crate::bookmarks::BookmarkNotice::BaseUnavailable { title },
                ));
            }
            Err(error) => {
                // As for PGN files in M2: only a file that is gone leaves
                // the recent list.
                if matches!(&error, BaseError::File(e) if e.kind == FileErrorKind::NotFound) {
                    self.prefs.files.recent.remove(&pending.path);
                    self.prefs_dirty = true;
                }
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(error)));
            }
            Ok(OpenOutcome::NoFreeSlot) => {
                // The answer was decided with the slots as they were when
                // the request left; a slot may have been closed since.
                if self.bases.first_free().is_some() {
                    self.start_open_base(pending.path, effects);
                } else {
                    self.notice = Some(AppNotice::Bases(BasesNotice::NoFreeSlot));
                }
            }
            Ok(OpenOutcome::AlreadyOpen(path)) => {
                self.remember_opened_file(path.clone());
                match self.bases.find_by_path(&path) {
                    Some(_) if self.switching_blocked() => {
                        self.notice = Some(AppNotice::Bases(BasesNotice::NotActivated));
                    }
                    Some(slot) => {
                        self.select_slot(slot);
                        if let Some(bookmark) = bookmark {
                            self.bookmark_search(slot, bookmark, effects);
                        }
                    }
                    // Closed while the request was in flight: open it again.
                    None => {
                        self.start_open_base(pending.path, effects);
                        if self.flow.open_base.is_some() {
                            self.flow.bookmark_go = bookmark;
                        }
                    }
                }
            }
            Ok(OpenOutcome::Opened(opened)) => {
                let Some(id) = self.bases.first_free() else {
                    self.notice = Some(AppNotice::Bases(BasesNotice::NoFreeSlot));
                    return;
                };
                self.remember_opened_file(opened.path.clone());
                let slot = BaseSlot::from_opened(opened);
                let single = slot.total() == 1;
                self.bases.install(id, slot);
                if self.switching_blocked() {
                    // A question about the game is open: switching now would
                    // change the game it is about. The slot stays ready.
                    self.notice = Some(AppNotice::Bases(BasesNotice::NotActivated));
                    return;
                }
                self.select_slot(id);
                if let Some(bookmark) = bookmark {
                    self.bookmark_search(id, bookmark, effects);
                    return;
                }
                // A file with one game loads it at once (D503).
                if single {
                    self.request_base_game(id, 1, None, effects);
                }
            }
        }
    }

    /// Whether the editor's game must not change under the user: a dialog
    /// that edits or replaces it, or a question about it, is open.
    pub(super) fn switching_blocked(&self) -> bool {
        self.modal_open() || self.overwrite_prompt.is_some()
    }

    /// Adds an opened file to the recent list, as the M2 open does.
    fn remember_opened_file(&mut self, path: PathBuf) {
        self.prefs.files.last_open_dir = path.parent().map(PathBuf::from);
        self.prefs.files.recent.push(path);
        self.prefs_dirty = true;
    }

    // ---- switching ----

    /// Makes `id` the current slot, keeping the game being edited in the
    /// slot it belonged to and restoring the one of `id`. Does nothing for
    /// the current slot or an empty one. Ends trial mode.
    pub(crate) fn select_slot(&mut self, id: SlotId) {
        if id == self.bases.current() || self.bases.slot(id).is_none() || self.refuse_edit() {
            return;
        }
        // The problem of the tactics training must not become the game of
        // the slot that is left: the game of the user goes back first.
        self.tactics_halt();
        self.park_current();
        self.activate(id);
    }

    fn select_next(&mut self, forward: bool) {
        let target = self.bases.cycle(self.bases.current(), forward);
        self.select_slot(target);
    }

    /// Stores the game of the editor in the current slot.
    fn park_current(&mut self) {
        if self.session.in_trial() {
            drop(self.session.update(GameMessage::ExitTrial));
        }
        let parked = Parked {
            game: self.session.game().clone(),
            origin: self.origin.clone(),
            cursor: self.session.cursor(),
            dirty: self.session.is_dirty(),
        };
        if let Some(slot) = self.bases.slot_mut(self.bases.current()) {
            slot.parked = Some(parked);
        }
    }

    /// Makes `id` current and puts its game in the editor: the one it had
    /// when it was left, or an empty game.
    fn activate(&mut self, id: SlotId) {
        self.bases.set_current(id);
        let parked = self.bases.slot_mut(id).and_then(|slot| slot.parked.take());
        if let Some(parked) = parked {
            self.session.replace_game(parked.game);
            if parked.dirty {
                self.session.mark_altered();
            }
            drop(
                self.session
                    .update(GameMessage::Navigate(Nav::GoToNode(parked.cursor))),
            );
            self.origin = parked.origin;
        } else {
            self.session.replace_game(Game::new());
            self.origin = GameOrigin::New;
        }
        self.collection = None;
        self.pasted_moves = None;
        self.chooser = None;
        // A game asked for another slot is no longer wanted.
        self.flow.base_game = None;
    }

    // ---- closing ----

    fn close_request(&mut self, id: SlotId, effects: &mut Vec<Effect>) {
        if id.is_clipbase() {
            // The Clipbase is never closed: "Close" is "Reset" (D507).
            self.clipbase_clear();
            return;
        }
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        // The altered game of another slot can only be saved from the
        // editor: bring that slot forward and ask about it there (D514).
        if id != self.bases.current() && slot.parked.as_ref().is_some_and(parked_needs_asking) {
            self.select_slot(id);
        }
        if id == self.bases.current() {
            self.tactics_halt();
            self.proceed_or_ask(Pending::CloseBase(id), effects);
        } else {
            self.close_slot_in_flow(id, effects);
        }
    }

    /// Closes a slot without asking. Closing the current one makes the
    /// Clipbase current, with the game it had.
    #[cfg(test)]
    pub(crate) fn close_slot(&mut self, id: SlotId) {
        self.close_slot_inner(id);
    }

    /// `AppState::close_slot` for the flows that have effects: a question
    /// about the quit that the closing dropped gives the quit up (D3511).
    pub(crate) fn close_slot_in_flow(&mut self, id: SlotId, effects: &mut Vec<Effect>) {
        if self.close_slot_inner(id) {
            self.flow.quit_asked.clear();
            self.tournament_quit_undone(effects);
            self.spelling_quit_undone(effects);
        }
    }

    /// Returns whether a question about the quit was dropped.
    fn close_slot_inner(&mut self, id: SlotId) -> bool {
        if self.refuse_edit() {
            return false;
        }
        if id.is_clipbase() {
            self.clipbase_clear();
            return false;
        }
        if id == self.bases.current() {
            self.tactics_halt();
        }
        if self.bases.remove(id).is_none() {
            return false;
        }
        if self.browser.as_ref().is_some_and(|b| b.slot == id) {
            self.close_browser();
        }
        // A job that reads or writes this slot is cancelled (a copy
        // between databases also when the closed slot is its target; the
        // games already written stay, D578).
        if self
            .flow
            .list_job
            .as_ref()
            .is_some_and(|j| j.slot == id || j.target.as_ref().is_some_and(|(to, _)| *to == id))
            && let Some(job) = self.flow.list_job.as_mut()
        {
            // The job stays until its thread answers: it may still be
            // writing the last batch and reopening the file (D588).
            job.control.cancel();
            job.closing = true;
        }
        self.merge.slot_gone(id);
        if self.flow.base_game.as_ref().is_some_and(|g| g.slot == id) {
            self.flow.base_game = None;
        }
        if id == self.bases.current() {
            self.activate(SlotId::CLIPBASE);
        }
        // The slot of the question about the quit is gone: the caller gives
        // the quit up like "Cancel" does (D3511).
        self.discard_prompt
            .take()
            .is_some_and(|p| matches!(p.pending, Pending::Quit))
    }

    // ---- games ----

    fn load_base_game(&mut self, id: SlotId, number: u32, effects: &mut Vec<Effect>) {
        let in_range = self
            .bases
            .slot(id)
            .is_some_and(|slot| number >= 1 && number <= slot.total());
        if !in_range {
            return;
        }
        // Loading from a list activates its base first (rule 11).
        self.select_slot(id);
        self.request_base_game(id, number, None, effects);
    }

    /// Reads game `number` of slot `id` for the editor; `landing` is set
    /// when the browser asked for it.
    pub(crate) fn request_base_game(
        &mut self,
        id: SlotId,
        number: u32,
        landing: Option<BrowserLanding>,
        effects: &mut Vec<Effect>,
    ) {
        // Reading a record takes the write lock: not while a job of the list
        // works on the same database (D570).
        if self
            .flow
            .list_job
            .as_ref()
            .is_some_and(|j| j.slot == id || j.target.as_ref().is_some_and(|(to, _)| *to == id))
        {
            self.notice = Some(AppNotice::Bases(BasesNotice::ListBusy));
            return;
        }
        let Some(slot) = self.bases.slot(id) else {
            return;
        };
        let source = slot.source.clone();
        // The game that arrives replaces the editor's: the problem of the
        // tactics training leaves first, with the game of the user.
        self.tactics_halt();
        let request = self.new_request();
        self.flow.base_game = Some(PendingBaseGame {
            request,
            slot: id,
            number,
            landing,
            names_refresh: false,
        });
        effects.push(Effect::LoadBaseGame {
            request,
            slot: id,
            number,
            source,
        });
    }

    fn base_game_loaded(
        &mut self,
        request: RequestId,
        result: Result<SharedGame, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.base_game.as_ref().map(|g| g.request) != Some(request) {
            return;
        }
        let Some(pending) = self.flow.base_game.take() else {
            return;
        };
        let game = match result {
            Ok(game) => game,
            Err(error) => {
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(error)));
                return;
            }
        };
        if pending.slot != self.bases.current() {
            return;
        }
        if self.discard_prompt.is_some() {
            self.notice = Some(AppNotice::Bases(BasesNotice::GameNotLoaded));
            return;
        }
        if pending.names_refresh && !self.names_reload_allowed() {
            // The game was edited (or taken over) while it was read: the
            // old names stay, and the user is told (D3472).
            self.notice = Some(AppNotice::Spelling(
                crate::spelling_ui::SpellingNotice::GameNotRefreshed,
            ));
            return;
        }
        let Some(slot) = self.bases.slot(pending.slot) else {
            return;
        };
        let origin = match (&slot.kind, &slot.path) {
            (BaseKind::Pgn, Some(path)) => GameOrigin::PgnFile {
                path: path.clone(),
                index: usize::try_from(pending.number.saturating_sub(1)).unwrap_or(0),
                total: usize::try_from(slot.total()).unwrap_or(usize::MAX),
                partial: slot.is_partial(),
            },
            // A game of an SI4 database or of the Clipbase is not a PGN
            // file: "Save" asks where to write it.
            _ => GameOrigin::New,
        };
        self.begin_replace(
            Replacement::FromBase {
                slot: pending.slot,
                number: pending.number,
                game,
                origin,
                landing: pending.landing,
            },
            effects,
        );
    }

    /// Records which game of the current slot the editor holds: `from` when
    /// it is a game of that slot, none otherwise (a new game, a pasted one).
    pub(crate) fn note_current_game(&mut self, from: Option<(SlotId, u32)>) {
        let current = self.bases.current();
        if let Some(slot) = self.bases.slot_mut(current) {
            slot.current_game = from.filter(|(id, _)| *id == current).map(|(_, n)| n);
            slot.parked = None;
        }
    }

    /// Goes on with a quit after the current game was dealt with (saved,
    /// discarded or clean): the next altered game of another slot is
    /// brought to the editor and asked about; when none is left the
    /// program quits (D525). The slot just dealt with goes to
    /// `flow.quit_asked` and is not asked again in this quit; its game keeps
    /// its altered flag, so a cancelled quit loses nothing (D549).
    pub(crate) fn continue_quit(&mut self, effects: &mut Vec<Effect>) {
        let current = self.bases.current();
        if !self.flow.quit_asked.contains(&current) {
            self.flow.quit_asked.push(current);
        }
        let next = self
            .bases
            .slots_with_altered_game()
            .into_iter()
            .find(|id| !self.flow.quit_asked.contains(id));
        if let Some(next) = next {
            self.select_slot(next);
            self.discard_prompt = Some(DiscardPrompt {
                pending: Pending::Quit,
            });
            return;
        }
        self.flow.quit_asked.clear();
        // The tree mask is asked about last (spec M5-4b, D1873).
        if !self.mask_quit_gate(effects) {
            self.finish_quit(effects);
        }
    }

    // ---- Clipbase ----

    /// Whether a list job works on the Clipbase (a search of its list, or a
    /// copy into it): the commands that take its write lock inside `update`
    /// wait for the job instead of making `update` and `view` wait for the
    /// lock (D570). Tells the user.
    pub(crate) fn clipbase_busy(&mut self) -> bool {
        let busy = self.flow.list_job.as_ref().is_some_and(|j| {
            j.slot.is_clipbase()
                || j.kind == ListJobKind::Copy
                || j.target.as_ref().is_some_and(|(to, _)| to.is_clipbase())
        });
        if busy {
            self.notice = Some(AppNotice::Bases(BasesNotice::ListBusy));
        }
        busy
    }

    /// Copies the game being edited to the end of the Clipbase (`sc_clipbase
    /// copy`). Encoding one game is the only work done in `update`.
    fn clipbase_copy(&mut self) {
        if self.clipbase_busy() {
            return;
        }
        if self.bases.current().is_clipbase() {
            self.notice = Some(AppNotice::Bases(BasesNotice::AlreadyInClipbase));
            return;
        }
        // Trial moves are not part of the game: leave the trial first, as a
        // save does.
        if self.session.in_trial() {
            drop(self.session.update(GameMessage::ExitTrial));
        }
        let game = self.session.game().clone();
        let Some(slot) = self.bases.slot_mut(SlotId::CLIPBASE) else {
            self.notice = Some(AppNotice::Bases(BasesNotice::Failed(BaseError::Internal)));
            return;
        };
        let pushed = slot
            .source
            .lock()
            .clipbase_mut()
            .map(|clip| clip.push_game(&game));
        match pushed {
            Some(Ok(number)) => {
                // The game enters the filter, like `append(1)` of the Scid.
                slot.filter.append(1);
                // The sorted view knows nothing of the new game.
                slot.clear_sort();
                slot.current_game = Some(number);
                // Edits made to a Clipbase game earlier are dropped: the
                // copy is the active game now (Scid: "clipbase changes get
                // discarded").
                slot.parked = Some(Parked {
                    cursor: game.root(),
                    game,
                    origin: GameOrigin::New,
                    dirty: false,
                });
            }
            Some(Err(error)) => {
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(error)));
            }
            None => {
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(BaseError::Internal)));
            }
        }
    }

    /// Replaces the game being edited with the active game of the Clipbase
    /// (`sc_clipbase paste`); nothing happens in the Clipbase itself. The
    /// pasted game counts as altered, like pasted text. Reading one game
    /// from memory is the only work done in `update` (D516).
    fn clipbase_paste(&mut self, effects: &mut Vec<Effect>) {
        if self.clipbase_busy() {
            return;
        }
        if self.bases.current().is_clipbase() {
            return;
        }
        let Some(slot) = self.bases.slot(SlotId::CLIPBASE) else {
            self.notice = Some(AppNotice::Bases(BasesNotice::Failed(BaseError::Internal)));
            return;
        };
        let total = slot.total();
        if total == 0 {
            self.notice = Some(AppNotice::Bases(BasesNotice::ClipbaseEmpty));
            return;
        }
        let number = slot
            .current_game
            .filter(|n| (1..=total).contains(n))
            .unwrap_or(total);
        let game = slot.source.lock().game(number);
        match game {
            Ok(game) => self.begin_replace(
                Replacement::Loaded {
                    game: SharedGame::new(game),
                    origin: GameOrigin::Clipboard,
                    dirty: true,
                    collection: None,
                },
                effects,
            ),
            Err(_) => {
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(
                    BaseError::GameUnreadable { number },
                )));
            }
        }
    }

    /// Removes every game of the Clipbase and resets its filter and list
    /// (`sc_clipbase clear`).
    pub(crate) fn clipbase_clear(&mut self) {
        if self.clipbase_busy() {
            return;
        }
        let Some(slot) = self.bases.slot_mut(SlotId::CLIPBASE) else {
            return;
        };
        let cleared = slot
            .source
            .lock()
            .clipbase_mut()
            .map_or(Err(BaseError::Internal), Clipbase::clear);
        match cleared {
            Ok(()) => {
                self.merge.slot_gone(SlotId::CLIPBASE);
                slot.filter = mychess_db::Filter::new(0);
                slot.list = crate::bases::ListState::default();
                slot.clear_sort();
                slot.current_game = None;
                slot.parked = None;
                // In the Clipbase the edited game is a Clipbase game
                // (`sc_clipbase_clear` clears it too).
                if self.bases.current().is_clipbase() {
                    self.session.replace_game(Game::new());
                    self.origin = GameOrigin::New;
                    self.collection = None;
                }
            }
            Err(error) => {
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(error)));
            }
        }
    }
}

/// Whether the parked game was altered and is worth a question: not just
/// an empty game with default tags (the rule of D103).
fn parked_needs_asking(parked: &Parked) -> bool {
    parked.dirty
        && (parked.game.mainline().next().is_some()
            || *parked.game.tags() != mychess_core::Tags::default())
}

impl Bases {
    /// The slots, other than the current one, whose parked game was altered
    /// and is worth a question, in order. The Clipbase is left out: its
    /// games are temporary (Scid: `::file::Exit`). Read-only slots are not
    /// left out, unlike the Scid: a game taken from a PGN can be saved to a
    /// file of its own, and quitting would lose it (D526).
    pub fn slots_with_altered_game(&self) -> Vec<SlotId> {
        self.iter()
            .filter(|(id, slot)| {
                *id != self.current()
                    && !id.is_clipbase()
                    && slot.parked.as_ref().is_some_and(parked_needs_asking)
            })
            .map(|(id, _)| id)
            .collect()
    }
}
