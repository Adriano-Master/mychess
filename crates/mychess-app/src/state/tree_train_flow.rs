//! The training of the opening tree, the lock on a database, "open base as
//! tree" and the best games section (spec M5-2, D1600-D1642).
//!
//! The training answers a move of the user with a move drawn from the tree of
//! the new position. The tree is searched asynchronously, so the hook only
//! records what it waits for ([`crate::tree::TrainingWaitInfo`]) and the answer is played when
//! the search of that position comes back ([`AppState::tree_try_answer`]).

use mychess_db::tree::random_move;

use super::AppState;
use crate::bases::{BasesMessage, BasesNotice, BrowseMessage, ListMessage, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::{AppNotice, BrowserLanding};
use crate::host::PickKind;
use crate::message::Message;
use crate::session::{GameMessage, PlayOptions};
use crate::training::{TrainingHook, TrainingOutcome};
use crate::tree::{BestMessage, TrainingWaitInfo};

/// The tree as a [`TrainingHook`]: it runs after the automove and before
/// anything else (D1622).
pub(super) struct TreeTrainingHook;

impl TrainingHook for TreeTrainingHook {
    fn after_user_move(
        &mut self,
        state: &mut AppState,
        effects: &mut Vec<Effect>,
    ) -> TrainingOutcome {
        if !state.tree.training || state.tree.answering {
            return TrainingOutcome::Continue;
        }
        state.tree.training_wait = Some(TrainingWaitInfo {
            fen: state.session.position().fen(),
            cursor: state.session.cursor(),
        });
        state.tree_try_answer(effects);
        TrainingOutcome::Handled
    }
}

impl AppState {
    /// The database the tree shows: the locked one, or the current one
    /// (D1630).
    pub(crate) fn tree_target(&self) -> SlotId {
        self.tree
            .locked
            .as_ref()
            .map_or_else(|| self.bases.current(), |(id, _)| *id)
    }

    /// Fixes the seed of the training generator (for tests, D1749).
    #[doc(hidden)]
    pub fn seed_tree_training(&mut self, seed: u64) {
        self.tree.seed_training(seed);
    }

    /// Asks the front-end to show the panel (D1641); the front-end calls
    /// this once per change.
    pub fn take_tree_show_request(&mut self) -> bool {
        std::mem::take(&mut self.tree.show_request)
    }

    /// The lock falls when its slot closes or holds another database.
    pub(super) fn tree_check_lock(&mut self) {
        let Some((id, source)) = &self.tree.locked else {
            return;
        };
        let same = self.bases.slot(*id).is_some_and(|s| s.source == *source);
        if !same {
            self.tree.locked = None;
            self.tree.last_key = None;
        }
    }

    pub(super) fn tree_set_locked(&mut self, on: bool) {
        if on {
            let id = self.bases.current();
            if let Some(slot) = self.bases.slot(id) {
                self.tree.locked = Some((id, slot.source.clone()));
            }
        } else {
            self.tree.locked = None;
        }
    }

    fn tree_lock_on(&mut self, id: SlotId) {
        // The tree is assistance: not while an online game locks it (D1742).
        if self.assist_locked() {
            return;
        }
        if let Some(slot) = self.bases.slot(id) {
            self.tree.locked = Some((id, slot.source.clone()));
        }
        self.tree_open();
        self.tree.show_request = true;
    }

    // ---- training ----

    /// Whether the training can be on now (it stays available while on, so
    /// it can be turned off).
    pub(super) fn tree_training_available(&self) -> bool {
        if self.tree.training {
            return true;
        }
        self.tree.open
            && self.tree.autorefresh
            && !self.edits_blocked()
            && !self.annotation_running()
            && !self.play_live()
            && !self.modal_open()
            && !self.training_playing()
            && self
                .bases
                .slot(self.tree_target())
                .is_some_and(|slot| slot.total() > 0 && !slot.needs_reload)
    }

    pub(super) fn tree_set_training(&mut self, on: bool, effects: &mut Vec<Effect>) {
        if on == self.tree.training {
            return;
        }
        if !on {
            self.tree_training_stop(true);
            return;
        }
        if !self.tree_training_available() {
            return;
        }
        if !self.session.in_trial() {
            effects.extend(
                self.session
                    .update(GameMessage::EnterTrial { null_move: false }),
            );
            self.tree.training_owns_trial = self.session.in_trial();
        }
        if self.session.in_trial() {
            self.tree.training = true;
        }
    }

    /// Ends the training; `exit_trial` also leaves the trial mode the
    /// training turned on.
    pub(super) fn tree_training_stop(&mut self, exit_trial: bool) {
        if !self.tree.training {
            return;
        }
        self.tree.training = false;
        self.tree.training_wait = None;
        let owned = std::mem::take(&mut self.tree.training_owns_trial);
        if exit_trial && owned && self.session.in_trial() {
            drop(self.session.update(GameMessage::ExitTrial));
        }
    }

    /// The training ends when its conditions go away (D1625).
    pub(super) fn tree_training_guard(&mut self) {
        if self.tree.training
            && (!self.session.in_trial()
                || !self.tree.open
                || !self.tree.autorefresh
                || self
                    .bases
                    .slot(self.tree_target())
                    .is_none_or(|slot| slot.total() == 0))
        {
            self.tree_training_stop(false);
        }
    }

    /// Drops the wait of the training when the search it waits for will
    /// never come: nothing is flying, the last search wanted was for that
    /// position, and what is on screen is not it (D1744). A search that is
    /// only deferred (a list job runs) has another key and keeps the wait.
    pub(super) fn tree_training_watchdog(&mut self) {
        let Some(wait) = &self.tree.training_wait else {
            return;
        };
        if self.tree.searching() {
            return;
        }
        let asked = self
            .tree
            .last_key
            .as_ref()
            .is_some_and(|key| key.fen == wait.fen);
        let shown = self.tree.shown.as_ref().is_some_and(|s| s.fen == wait.fen);
        if asked && !shown {
            self.tree.training_wait = None;
        }
    }

    /// Plays the drawn answer if the search of the position the training
    /// waits for is on screen (D1623).
    pub(crate) fn tree_try_answer(&mut self, effects: &mut Vec<Effect>) {
        let Some(wait) = self.tree.training_wait.clone() else {
            return;
        };
        let position = self.session.position().clone();
        if !self.tree.training || self.session.cursor() != wait.cursor || position.fen() != wait.fen
        {
            self.tree.training_wait = None;
            return;
        }
        if self.tree.searching() {
            return;
        }
        let Some(shown) = self.tree.shown.as_ref().filter(|s| s.fen == wait.fen) else {
            return;
        };
        let result = std::sync::Arc::clone(&shown.result);
        self.tree.training_wait = None;
        let picked = random_move(&result, &mut || self.tree.next_random());
        let Some(mv) = picked.and_then(|m| position.find_move(m.from, m.to, m.promotion)) else {
            return;
        };
        self.tree.answering = true;
        effects.extend(super::update(
            self,
            Message::Game(GameMessage::PlayMove {
                mv,
                options: PlayOptions::default(),
            }),
        ));
        self.tree.answering = false;
    }

    // ---- open base as tree ----

    pub(super) fn tree_ask_base(&mut self, effects: &mut Vec<Effect>) {
        self.ask_for_file(PickKind::OpenTreeBase, None, effects);
    }

    /// The file was chosen (D1640).
    pub(crate) fn tree_open_base_as_tree(
        &mut self,
        path: std::path::PathBuf,
        effects: &mut Vec<Effect>,
    ) {
        if self.assist_locked() {
            self.tree.open_as_tree = None;
            return;
        }
        if let Some(slot) = self.bases.find_by_path(&path) {
            self.tree_lock_on(slot);
            return;
        }
        self.tree.open_as_tree = Some(path.clone());
        self.start_open_base(path, effects);
        if self.flow.open_base.is_none() {
            self.tree.open_as_tree = None;
        }
    }

    /// The base the file asked for is open: lock the tree on it and give the
    /// editor back the base it had (Scid `SwitchToBase $oldbase`).
    pub(crate) fn tree_after_open(&mut self, before: SlotId) {
        if self.flow.open_base.is_some() {
            return;
        }
        let Some(path) = self.tree.open_as_tree.take() else {
            return;
        };
        let current = self.bases.current();
        let slot = if current == before {
            self.bases.find_by_path(&path)
        } else {
            Some(current)
        };
        let Some(slot) = slot else {
            return;
        };
        self.tree_lock_on(slot);
        if slot != before {
            self.select_slot(before);
        }
    }

    // ---- best games ----

    pub(super) fn tree_best_message(&mut self, message: &BestMessage, effects: &mut Vec<Effect>) {
        match message {
            BestMessage::Toggle => {
                let best = &mut self.tree.best;
                best.open = !best.open;
                if best.open {
                    return;
                }
                best.clear();
                best.columns_open = false;
                // Closing needs no new search.
                if let Some(key) = self.tree.last_key.as_mut() {
                    key.best = None;
                }
            }
            BestMessage::SetResults(results) => self.tree.best.results = *results,
            BestMessage::SetOrder(order) => self.tree.best.order = *order,
            BestMessage::SetMax(max) => self.tree.best.max = (*max).clamp(1, 1000),
            BestMessage::ToggleColumn(column) => self.tree.best.toggle_column(*column),
            BestMessage::ToggleColumnMenu => {
                self.tree.best.columns_open = !self.tree.best.columns_open;
            }
            BestMessage::Pick { number, mode } => self.tree.best.pick(*number, *mode),
            BestMessage::Load(number) => self.best_load(*number, effects),
            BestMessage::Browse => {
                let numbers = self.tree.best.selected_in_order();
                let Some(slot) = self.tree.best.slot else {
                    return;
                };
                if numbers.is_empty() {
                    self.notice = Some(AppNotice::Bases(BasesNotice::NothingSelected));
                    return;
                }
                effects.extend(super::update(
                    self,
                    Message::Bases(BasesMessage::Browse(BrowseMessage::Open { slot, numbers })),
                ));
            }
            BestMessage::Copy => self.best_copy(effects),
        }
    }

    fn best_load(&mut self, number: u32, effects: &mut Vec<Effect>) {
        let Some(slot) = self.tree.best.slot else {
            return;
        };
        let Some(ply) = self.tree.best.ply_of(number) else {
            return;
        };
        if self.switching_blocked()
            || !self
                .bases
                .slot(slot)
                .is_some_and(|s| number >= 1 && number <= s.total())
        {
            return;
        }
        // Before switching: a refused read must not leave the slot changed
        // (D1745).
        if self
            .flow
            .list_job
            .as_ref()
            .is_some_and(|j| j.slot == slot || j.target.as_ref().is_some_and(|(to, _)| *to == slot))
        {
            self.notice = Some(AppNotice::Bases(BasesNotice::ListBusy));
            return;
        }
        self.select_slot(slot);
        let landing = BrowserLanding {
            ply: usize::from(ply),
            close_browser: false,
            flipped: self.board_flipped,
            browser: RequestId(0),
        };
        self.request_base_game(slot, number, Some(landing), effects);
    }

    /// Copies the selected games through the list's own copy, lending it the
    /// selection (D1608).
    fn best_copy(&mut self, effects: &mut Vec<Effect>) {
        let Some(slot) = self.tree.best.slot else {
            return;
        };
        if self.tree.best.selected.is_empty() {
            self.notice = Some(AppNotice::Bases(BasesNotice::NothingSelected));
            return;
        }
        let wanted = self.tree.best.selected.clone();
        let Some(base) = self.bases.slot_mut(slot) else {
            return;
        };
        let saved = (
            std::mem::replace(&mut base.list.selection, wanted),
            base.list.selected,
        );
        effects.extend(super::update(
            self,
            Message::Bases(BasesMessage::List(ListMessage::CopySelected { slot })),
        ));
        if let Some(base) = self.bases.slot_mut(slot) {
            base.list.selection = saved.0;
            base.list.selected = saved.1;
        }
    }
}

#[cfg(test)]
#[allow(clippy::large_futures)] // the test rig owns the whole `AppState` across awaits
mod tests {
    //! Sanity tests of the training, the lock and the best games. The full
    //! contract suite is the tester's.

    use std::collections::VecDeque;

    use super::*;
    use crate::bases::{BasesMessage as Bm, PickMode};
    use crate::effect::perform;
    use crate::state::update;
    use crate::tree::{BestOrder, BestResults, TreeBody, TreeMessage};

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
    async fn opened(games: &str, name: &str) -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(name);
        std::fs::write(&path, games).expect("write");
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        drive(&mut state, Bm::Open(path)).await;
        (state, dir)
    }

    fn play_uci(state: &mut AppState, uci: &str) -> Message {
        let mv = state.session.position().parse_uci(uci).expect("legal");
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        })
    }

    #[tokio::test]
    async fn best_games_follow_the_results_and_the_selection() {
        let (mut state, _dir) = opened(GAMES, "g.pgn").await;
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        assert!(state.tree_view().best.is_none());
        drive(
            &mut state,
            Message::Tree(TreeMessage::Best(BestMessage::Toggle)),
        )
        .await;
        let view = state.tree_view();
        let best = view.best.expect("open");
        // Every game of the start position, last ones in rating order (all 0).
        assert_eq!(best.rows.len(), 5);
        drive(
            &mut state,
            Message::Tree(TreeMessage::Best(BestMessage::SetResults(
                BestResults::White,
            ))),
        )
        .await;
        let best = state.tree_view().best.expect("open");
        let numbers: Vec<u32> = best.rows.iter().map(|r| r.game.number).collect();
        assert_eq!(numbers, [1, 4]);
        drive(
            &mut state,
            Message::Tree(TreeMessage::Best(BestMessage::SetOrder(
                BestOrder::Database,
            ))),
        )
        .await;
        drive(
            &mut state,
            Message::Tree(TreeMessage::Best(BestMessage::Pick {
                number: 4,
                mode: PickMode::Only,
            })),
        )
        .await;
        let best = state.tree_view().best.expect("open");
        assert_eq!(best.selected.iter().copied().collect::<Vec<_>>(), [4]);
        // Closing the section forgets the list without a new search.
        let started = state.tree().searches_started();
        drive(
            &mut state,
            Message::Tree(TreeMessage::Best(BestMessage::Toggle)),
        )
        .await;
        assert!(state.tree_view().best.is_none());
        assert_eq!(state.tree().searches_started(), started);
    }

    #[tokio::test]
    async fn copy_without_selection_only_notifies() {
        let (mut state, _dir) = opened(GAMES, "g.pgn").await;
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        drive(
            &mut state,
            Message::Tree(TreeMessage::Best(BestMessage::Toggle)),
        )
        .await;
        let effects = update(
            &mut state,
            Message::Tree(TreeMessage::Best(BestMessage::Copy)),
        );
        assert!(effects.is_empty());
        assert!(matches!(
            state.notice,
            Some(AppNotice::Bases(BasesNotice::NothingSelected))
        ));
    }

    #[tokio::test]
    async fn training_answers_the_move_of_the_user_and_stops_with_the_trial() {
        let (mut state, _dir) = opened(GAMES, "g.pgn").await;
        state.tree.seed_training(7);
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        drive(&mut state, Message::Tree(TreeMessage::SetTraining(true))).await;
        assert!(state.tree().training());
        assert!(state.session.in_trial());
        let view = state.tree_view();
        assert!(view.rows.iter().all(|r| r.hidden && !r.clickable));
        let message = play_uci(&mut state, "e2e4");
        drive(&mut state, message).await;
        // The user's e4 and the answer (e5 or c5): two plies.
        assert_eq!(state.session.game().ply(state.session.cursor()), Ok(2));
        assert!(state.tree().training());
        // Leaving the trial ends the training.
        drive(&mut state, Message::Game(GameMessage::ExitTrial)).await;
        assert!(!state.tree().training());
    }

    #[tokio::test]
    async fn training_does_not_answer_without_moves_in_the_tree() {
        let (mut state, _dir) = opened(GAMES, "g.pgn").await;
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        drive(&mut state, Message::Tree(TreeMessage::SetTraining(true))).await;
        // No game plays 1.a3.
        let message = play_uci(&mut state, "a2a3");
        drive(&mut state, message).await;
        assert_eq!(state.session.game().ply(state.session.cursor()), Ok(1));
        assert_eq!(state.tree_view().body, TreeBody::Empty);
    }

    #[tokio::test]
    async fn closing_the_panel_ends_the_training_and_the_trial() {
        let (mut state, _dir) = opened(GAMES, "g.pgn").await;
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        drive(&mut state, Message::Tree(TreeMessage::SetTraining(true))).await;
        drive(&mut state, Message::Tree(TreeMessage::Close)).await;
        assert!(!state.tree().training());
        assert!(!state.session.in_trial());
    }

    #[tokio::test]
    async fn training_needs_the_panel_open() {
        let (mut state, _dir) = opened(GAMES, "g.pgn").await;
        drive(&mut state, Message::Tree(TreeMessage::SetTraining(true))).await;
        assert!(!state.tree().training());
        assert!(!state.session.in_trial());
    }

    #[tokio::test]
    async fn a_locked_tree_keeps_its_base_and_falls_when_it_closes() {
        let (mut state, dir) = opened(GAMES, "a.pgn").await;
        let first = state.bases.current();
        let other = dir.path().join("b.pgn");
        std::fs::write(&other, "[Result \"1-0\"]\n\n1. c4 c5 1-0\n\n").expect("write");
        drive(&mut state, Bm::Open(other)).await;
        let second = state.bases.current();
        assert_ne!(first, second);
        drive(&mut state, Bm::Select(first)).await;
        drive(&mut state, Message::Tree(TreeMessage::Open)).await;
        drive(&mut state, Message::Tree(TreeMessage::SetLocked(true))).await;
        assert!(state.tree_view().locked);
        drive(&mut state, Bm::Select(second)).await;
        assert_eq!(state.tree_target(), first);
        let view = state.tree_view();
        assert_eq!(view.total.as_ref().map(|t| t.games), Some(5));
        assert_eq!(view.target_name.as_deref(), Some("a"));
        drive(&mut state, Message::Tree(TreeMessage::SetLocked(false))).await;
        assert_eq!(state.tree_target(), second);
        assert_eq!(state.tree_view().total.as_ref().map(|t| t.games), Some(1));
        // Locked again, then the locked base closes: the lock falls.
        drive(&mut state, Message::Tree(TreeMessage::SetLocked(true))).await;
        drive(&mut state, Bm::Close(second)).await;
        assert!(!state.tree().locked());
    }

    #[tokio::test]
    async fn open_base_as_tree_locks_on_the_file_and_keeps_the_current_base() {
        let (mut state, dir) = opened(GAMES, "a.pgn").await;
        let first = state.bases.current();
        let other = dir.path().join("b.pgn");
        std::fs::write(&other, "[Result \"1-0\"]\n\n1. c4 c5 1-0\n\n").expect("write");
        let mut effects = Vec::new();
        state.tree_open_base_as_tree(other, &mut effects);
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            if effect.runs_on_host() {
                continue;
            }
            let answer = perform(effect).await;
            queue.extend(update(&mut state, answer));
        }
        assert_eq!(state.bases.current(), first);
        assert!(state.tree().locked());
        assert!(state.tree().is_open());
        assert!(state.take_tree_show_request());
        assert!(!state.take_tree_show_request());
        assert_ne!(state.tree_target(), first);
    }
}
