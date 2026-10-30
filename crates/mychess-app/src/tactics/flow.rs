//! The tactics training in the application state (spec M8-4, phase b).
//!
//! The rules are in [`machine`](super::machine) (pure). This is the glue: it
//! hands the machine what it needs from the application (the current
//! database, the preferences path), runs what the machine asks for (effects,
//! the problem and its moves in the editor, saving the store), keeps the
//! game of the user parked while the problem is on (D1929), turns the moves
//! of the user into judgements (D1930) and enforces the exclusion of the
//! other modes (D1931). Nothing here blocks (D1936).

use std::sync::Arc;

use mychess_core::{Color, Game, Move, NodeId, Position};

use super::config::{SourceKind, TacticsConfig};
use super::machine::{Out, Tactics, TacticsPhase, TacticsView};
use super::message::{TacticsEffect, TacticsMessage, TacticsNotice, TacticsStartRequest};
use super::opinion::OpinionEngine;
use super::source::{OpenedSource, SourceHandle, source_key};
use super::store::store_path;
use crate::bases::{SharedSource, SlotId};
use crate::effect::Effect;
use crate::engines::{EngineId, Hotkey, Protocol};
use crate::files::{GameOrigin, PgnCollection};
use crate::flow::Pending;
use crate::play::{ColorChoice, PlayConfig, PlayMessage, StartFrom};
use crate::session::{GameMessage, Nav, PlayOptions};
use crate::state::AppState;

/// The game that the editor held before the problem took its place (D1929).
/// Shared with the calculation exercise (spec M8-6, D2320).
#[derive(Debug, Clone)]
pub(crate) struct Parked {
    pub(crate) game: Game,
    pub(crate) origin: GameOrigin,
    pub(crate) cursor: NodeId,
    pub(crate) flipped: bool,
    /// The game had changes that were not saved (they come back flagged).
    pub(crate) dirty: bool,
    /// The PGN collection the game was taken from, so "next game" still
    /// works when it comes back (D2360).
    pub(crate) collection: Option<Arc<PgnCollection>>,
}

impl Parked {
    /// The game of the editor as it is now.
    pub(crate) fn capture(state: &AppState) -> Parked {
        Parked {
            game: state.session.game().clone(),
            origin: state.origin.clone(),
            cursor: state.session.cursor(),
            flipped: state.board_flipped,
            dirty: state.session.is_dirty(),
            collection: state.collection.clone(),
        }
    }

    /// Whether letting this game go would lose something: it has unsaved
    /// changes and is not just an empty game with default tags (the rule of
    /// `needs_confirmation`, D2165).
    pub(crate) fn at_stake(&self) -> bool {
        self.dirty
            && (self.game.mainline().next().is_some()
                || *self.game.tags() != mychess_core::Tags::default())
    }
}

/// The database that a session of the current base reads from (D1953): the
/// session ends when the slot is closed or the database changes under it.
#[derive(Debug, Clone)]
struct BaseRef {
    slot: SlotId,
    source: SharedSource,
    total: u32,
}

/// What the glue remembers between messages.
#[derive(Debug, Default)]
pub(crate) struct TacticsFlow {
    pub(crate) machine: Tactics,
    parked: Option<Parked>,
    base: Option<BaseRef>,
    /// Effects of a session that ended in the middle of another message
    /// (a slot switch): they leave with the effects of that message.
    deferred: Vec<Effect>,
    /// The generation of the editor game that holds the problem: any other
    /// generation means the user put something else in the editor, which
    /// must not be overwritten.
    generation: Option<u64>,
    save_in_flight: bool,
    resave: bool,
    /// The program waits for the write of the score before it exits.
    pub(crate) quit_wait: bool,
}

impl AppState {
    /// A picture of the training for the front-end.
    pub fn tactics_view(&self) -> TacticsView {
        self.flow.tactics.machine.view()
    }

    /// Whether the configuration dialog of the training is open.
    pub fn tactics_dialog_open(&self) -> bool {
        self.flow.tactics.machine.dialog_open()
    }

    /// Whether a tactics session is on.
    pub fn tactics_live(&self) -> bool {
        self.flow.tactics.machine.is_live()
    }

    /// Whether the front-end must send [`TacticsMessage::Tick`].
    pub fn tactics_wants_ticks(&self) -> bool {
        self.flow.tactics.machine.phase().wants_ticks()
    }

    /// The phase of the training.
    pub fn tactics_phase(&self) -> TacticsPhase {
        self.flow.tactics.machine.phase()
    }

    /// Whether another mode owns the board or the engines (D1931).
    pub fn tactics_blocked(&self) -> bool {
        self.play_live()
            || self.tournament_live()
            || self.annotation_running()
            || self.training_playing()
            || self.report.review.is_some()
            || self.assist_locked()
            || self.opening_live()
            || self.calvar_live()
    }

    /// Whether a start is refused: another mode owns the board, or the
    /// editor is in trial mode, whose backup the training would destroy by
    /// replacing the game (D2377).
    fn tactics_start_refused(&self) -> bool {
        self.tactics_blocked() || self.session.in_trial()
    }

    pub(crate) fn tactics_editor_is_ours(&self) -> bool {
        self.flow.tactics.generation == Some(self.session.generation())
    }

    pub(crate) fn tactics_message(&mut self, message: TacticsMessage, effects: &mut Vec<Effect>) {
        match message {
            // A session that is on keeps the panel, not the dialog (D1937,
            // D1949): the front-end gives the focus back to the panel.
            TacticsMessage::Open => {
                if !self.flow.tactics.machine.is_live() {
                    self.flow.tactics.machine.open_dialog();
                    self.tactics_load_store(effects);
                }
            }
            TacticsMessage::CloseDialog => self.flow.tactics.machine.close_dialog(),
            TacticsMessage::Start { config, seed } => self.tactics_start(*config, seed, effects),
            TacticsMessage::Hint => self.flow.tactics.machine.hint_step(),
            TacticsMessage::ShowSolution => {
                let outs = self.flow.tactics.machine.show_solution();
                self.tactics_run(outs, effects);
            }
            TacticsMessage::Next => {
                let outs = self.flow.tactics.machine.next_problem();
                self.tactics_run(outs, effects);
            }
            TacticsMessage::Retry => {
                let outs = self.flow.tactics.machine.retry();
                self.tactics_run(outs, effects);
            }
            TacticsMessage::Close => self.tactics_stop(effects),
            TacticsMessage::ResetStats(config) => {
                let key = self.tactics_key_of(&config);
                let outs = self.flow.tactics.machine.reset(config.mode, key.as_deref());
                self.tactics_run(outs, effects);
                // The reset holds in memory but cannot be written: say so
                // instead of dropping the write silently (D1954).
                if self.flow.tactics.machine.store_read_only() {
                    self.flow
                        .tactics
                        .machine
                        .set_notice(TacticsNotice::StoreNotSaved);
                }
            }
            TacticsMessage::Tick(now) => {
                let outs = self.flow.tactics.machine.tick(now);
                self.tactics_run(outs, effects);
            }
            TacticsMessage::StoreLoaded(loaded) => self.flow.tactics.machine.store_loaded(*loaded),
            TacticsMessage::StoreSaved(result) => {
                self.flow.tactics.machine.store_saved(&result);
                self.flow.tactics.save_in_flight = false;
                if std::mem::take(&mut self.flow.tactics.resave) {
                    self.tactics_save(effects);
                }
                // The program was waiting for this write to exit (D1958).
                if self.flow.tactics.quit_wait && !self.flow.tactics.save_in_flight {
                    self.flow.tactics.quit_wait = false;
                    self.finish_quit(effects);
                }
            }
            TacticsMessage::Opened { request, result } => {
                let outs = self.flow.tactics.machine.opened(request, result);
                self.tactics_run(outs, effects);
            }
            TacticsMessage::Found { request, outcome } => {
                let outs = self.flow.tactics.machine.found(request, *outcome);
                self.tactics_run(outs, effects);
            }
            TacticsMessage::Opinion { request, verdict } => {
                let outs = self.flow.tactics.machine.opinion(request, *verdict);
                self.tactics_run(outs, effects);
            }
            TacticsMessage::Continue => self.tactics_continue(effects),
            TacticsMessage::Judged { request, verdict } => {
                let outs = self.flow.tactics.machine.judged(request, *verdict);
                self.tactics_run(outs, effects);
            }
        }
    }

    /// Whether the exit must wait for the score being written; asks for a
    /// deadline of five seconds the first time (D1958).
    pub(crate) fn tactics_quit_wait(&mut self, effects: &mut Vec<Effect>) -> bool {
        if self.opening_quit_wait(effects)
            || self.calvar_quit_wait(effects)
            || self.tournament_quit_wait(effects)
        {
            return true;
        }
        if !self.flow.tactics.save_in_flight {
            return false;
        }
        if !self.flow.tactics.quit_wait {
            self.flow.tactics.quit_wait = true;
            effects.push(Effect::QuitDeadline {
                delay: std::time::Duration::from_secs(5),
            });
        }
        true
    }

    /// Whether the quit waits for the write of the score (or for another
    /// write that `tactics_quit_wait` covers).
    pub(crate) fn tactics_quit_waiting(&self) -> bool {
        self.flow.tactics.quit_wait
    }

    /// The deadline of [`AppState::tactics_quit_wait`] ran out: the write
    /// is given up. Returns whether the exit was waiting.
    pub(crate) fn tactics_quit_deadline(&mut self) -> bool {
        // Both waits end at the deadline (D2233).
        let opening = self.opening_quit_deadline()
            | self.calvar_quit_deadline()
            | self.tournament_quit_deadline();
        if !std::mem::take(&mut self.flow.tactics.quit_wait) {
            return opening;
        }
        self.flow.tactics.save_in_flight = false;
        true
    }

    /// Reads `tactics.toml` once (D1926).
    fn tactics_load_store(&mut self, effects: &mut Vec<Effect>) {
        if self.flow.tactics.machine.store_requested() {
            return;
        }
        let Some(prefs) = self.flow.prefs_path.as_deref() else {
            return;
        };
        let path = store_path(prefs);
        self.flow.tactics.machine.begin_load();
        effects.push(Effect::Tactics(TacticsEffect::LoadStore { path }));
    }

    /// The key of the source that `config` names (D1926), without any I/O.
    fn tactics_key_of(&self, config: &TacticsConfig) -> Option<String> {
        match config.source {
            SourceKind::File => config.file.as_deref().map(source_key),
            SourceKind::CurrentBase => self
                .bases
                .slot(self.bases.current())
                .and_then(|slot| slot.path.as_deref())
                .map(source_key),
        }
    }

    /// Whether the training holds a game of the user with unsaved changes
    /// (M8-7, D2466).
    pub(crate) fn tactics_parked_at_stake(&self) -> bool {
        self.flow
            .tactics
            .parked
            .as_ref()
            .is_some_and(Parked::at_stake)
    }

    /// The position the problem on the board started from (M8-7, D2459).
    pub(crate) fn tactics_problem_position(&self) -> Option<Position> {
        let phase = self.flow.tactics.machine.phase();
        if !matches!(
            phase,
            TacticsPhase::Solving
                | TacticsPhase::Judging
                | TacticsPhase::Solved(_)
                | TacticsPhase::Failed(_)
        ) {
            return None;
        }
        self.flow.tactics.machine.problem_position().cloned()
    }

    /// The games of the filter of the current base as a source (D1917).
    pub(crate) fn tactics_current_base(&self) -> Option<OpenedSource> {
        let slot = self.bases.slot(self.bases.current())?;
        let numbers: Vec<u32> = slot.filter.iter_games().collect();
        Some(OpenedSource {
            handle: SourceHandle::Base {
                source: slot.source.clone(),
                numbers: Arc::new(numbers),
            },
            key: slot.path.as_deref().map(source_key).unwrap_or_default(),
            stamp: None,
        })
    }

    fn tactics_start(&mut self, config: TacticsConfig, seed: u64, effects: &mut Vec<Effect>) {
        // Never over a session that is on: it would lose the parked game
        // and the dialog cannot be opened meanwhile (D1949).
        if self.flow.tactics.machine.is_live() {
            return;
        }
        self.tactics_load_store(effects);
        if self.tactics_start_refused() {
            self.flow.tactics.machine.set_notice(TacticsNotice::Busy);
            return;
        }
        // The game of the editor is parked, not saved: the user is asked
        // first whether to save it (D2145). "Don't save" parks it with its
        // changes, which come back marked as unsaved (D1960): nothing is
        // lost.
        if !self.tactics_editor_is_ours() && self.needs_confirmation() {
            let request = TacticsStartRequest { config, seed };
            self.proceed_or_ask(Pending::TacticsStart(Box::new(request)), effects);
            return;
        }
        self.tactics_begin(config, seed, effects);
    }

    /// The start after the question about the game was answered (D2145).
    pub(crate) fn tactics_start_confirmed(
        &mut self,
        config: TacticsConfig,
        seed: u64,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.tactics.machine.is_live() {
            return;
        }
        if self.tactics_start_refused() {
            self.flow.tactics.machine.set_notice(TacticsNotice::Busy);
            return;
        }
        self.tactics_begin(config, seed, effects);
    }

    /// The engine that gives the second opinion (D2131): the one of the
    /// analysis key F2, else the first UCI engine of the list; none when the
    /// option is off, the help is locked or there is no such engine.
    fn tactics_opinion_engine(&self, config: &TacticsConfig) -> Option<OpinionEngine> {
        if !config.second_opinion || self.assist_locked() {
            return None;
        }
        let list = &self.engines;
        let entry = list
            .hotkey_owner(Hotkey::F2)
            .and_then(|id| list.get(id))
            .filter(|e| e.protocol == Protocol::Uci)
            .or_else(|| list.engines().iter().find(|e| e.protocol == Protocol::Uci))?;
        Some(OpinionEngine(
            entry.to_config(&self.prefs.analysis.run_defaults()),
        ))
    }

    fn tactics_begin(&mut self, config: TacticsConfig, seed: u64, effects: &mut Vec<Effect>) {
        let opinion = self.tactics_opinion_engine(&config);
        self.flow.tactics.machine.set_opinion_engine(opinion);
        let base = (config.source == SourceKind::CurrentBase)
            .then(|| self.tactics_current_base())
            .flatten();
        let base_ref = base.as_ref().and_then(|_| {
            let slot = self.bases.slot(self.bases.current())?;
            Some(BaseRef {
                slot: self.bases.current(),
                source: slot.source.clone(),
                total: slot.total(),
            })
        });
        // The machine validates the source before it changes anything
        // (D1949): a refusal leaves the editor and the dialog as they were.
        let outs = self.flow.tactics.machine.start(config, seed, base);
        if self.flow.tactics.machine.is_live() {
            self.flow.tactics.base = base_ref;
            self.tactics_stop_engines(effects);
        }
        self.tactics_run(outs, effects);
    }

    /// The engines would show the answer (D1931): they are stopped.
    pub(crate) fn tactics_stop_engines(&mut self, effects: &mut Vec<Effect>) {
        let runs: Vec<_> = self.engine_runs().iter().map(|r| r.id).collect();
        for run in runs {
            self.stop_run(run, effects);
        }
    }

    /// Ends the session in the middle of another message, putting the game
    /// of the user back at once; the effects leave with that message
    /// (D1952).
    pub(crate) fn tactics_halt(&mut self) {
        // The single stopping point of the training modes (D2233): the
        // opening trainer leaves wherever the tactics training does.
        self.opening_halt();
        self.calvar_halt();
        if !self.flow.tactics.machine.is_live() {
            return;
        }
        let mut effects = Vec::new();
        self.tactics_stop(&mut effects);
        self.flow.tactics.deferred.extend(effects);
    }

    /// Ends the session and puts the game of the user back (D1935).
    pub(crate) fn tactics_stop(&mut self, effects: &mut Vec<Effect>) {
        self.opening_stop(effects);
        self.calvar_stop(effects);
        let outs = self.flow.tactics.machine.close();
        self.tactics_run(outs, effects);
    }

    fn tactics_run(&mut self, outs: Vec<Out>, effects: &mut Vec<Effect>) {
        let mut save = false;
        for out in outs {
            match out {
                Out::Effect(effect) => effects.push(Effect::Tactics(effect)),
                Out::ShowProblem(position) => self.tactics_show(&position),
                Out::Play(mv) => self.tactics_play(mv),
                Out::AddLine(moves) => self.tactics_add_line(&moves),
                Out::SaveStore => save = true,
                Out::Closed => self.tactics_restore(),
                Out::Released => self.tactics_release(),
            }
        }
        if save {
            self.tactics_save(effects);
        }
    }

    /// Writes the store, one write at a time so that an old one never lands
    /// after a new one.
    fn tactics_save(&mut self, effects: &mut Vec<Effect>) {
        // A file of a newer version, or one that could not be read, is not
        // written over (D1954).
        if self.flow.tactics.machine.store_read_only() {
            return;
        }
        let Some(prefs) = self.flow.prefs_path.as_deref() else {
            return;
        };
        if self.flow.tactics.save_in_flight {
            self.flow.tactics.resave = true;
            return;
        }
        let path = store_path(prefs);
        self.flow.tactics.save_in_flight = true;
        effects.push(Effect::Tactics(TacticsEffect::SaveStore {
            path,
            store: Box::new(self.flow.tactics.machine.store().clone()),
        }));
    }

    /// Puts the problem in the editor, parking the game of the user the
    /// first time (D1929), and turns the board to the side that moves.
    fn tactics_show(&mut self, position: &Position) {
        if !self.tactics_editor_is_ours() {
            self.flow.tactics.parked = Some(Parked::capture(self));
        }
        let mut game = Game::from_position(position.clone());
        "Tactics".clone_into(&mut game.tags_mut().event);
        self.session.replace_game(game);
        self.flow.tactics.generation = Some(self.session.generation());
        self.origin = GameOrigin::New;
        self.collection = None;
        self.board_flipped = position.turn() == Color::Black;
        self.setup = None;
        self.info_dialog = None;
        self.chooser = None;
        self.discard_prompt = None;
    }

    fn tactics_play(&mut self, mv: Move) {
        if !self.tactics_editor_is_ours() {
            return;
        }
        drop(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
        drop(self.session.update(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
        self.session.mark_saved();
    }

    /// Adds the line to the main line and goes back to where it starts, so
    /// the user can step through it (D1920).
    fn tactics_add_line(&mut self, moves: &[Move]) {
        if !self.tactics_editor_is_ours() {
            return;
        }
        drop(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
        let from = self.session.cursor();
        for mv in moves {
            drop(self.session.update(GameMessage::PlayMove {
                mv: *mv,
                options: PlayOptions::default(),
            }));
        }
        drop(
            self.session
                .update(GameMessage::Navigate(Nav::GoToNode(from))),
        );
        self.session.mark_saved();
    }

    /// "Continue against the engine" (D2140): the game of the editor stays
    /// as it is and a game against the engine starts from its position.
    fn tactics_continue(&mut self, effects: &mut Vec<Effect>) {
        let view = self.flow.tactics.machine.view();
        if !view.can_continue {
            return;
        }
        // The game that was set aside with unsaved changes (D2145) would be
        // lost: the user closes the training first to get it back (D2165).
        if self
            .flow
            .tactics
            .parked
            .as_ref()
            .is_some_and(Parked::at_stake)
        {
            self.flow
                .tactics
                .machine
                .set_notice(TacticsNotice::ParkedUnsaved);
            return;
        }
        // The refusals that `play_start` would make are made here, before
        // the session is let go of (D2162).
        if self.session.in_trial()
            || self.modal_open()
            || self.flow.open_base.is_some()
            || self.training_playing()
        {
            self.flow.tactics.machine.set_notice(TacticsNotice::Busy);
            return;
        }
        // Only a UCI engine can play (D2162), from the three places.
        let list = &self.engines;
        let uci = |id: EngineId| {
            list.get(id)
                .is_some_and(|entry| entry.protocol == Protocol::Uci)
        };
        let engine = self
            .prefs
            .play
            .engine
            .map(EngineId)
            .filter(|id| uci(*id))
            .or_else(|| list.hotkey_owner(Hotkey::F2).filter(|id| uci(*id)))
            .or_else(|| {
                list.engines()
                    .iter()
                    .find(|e| e.protocol == Protocol::Uci)
                    .map(|e| e.id)
            });
        let Some(engine) = engine else {
            self.flow
                .tactics
                .machine
                .set_notice(TacticsNotice::NoEngineToPlay);
            return;
        };
        let mover = self.session.position().turn();
        let outs = self.flow.tactics.machine.release();
        self.tactics_run(outs, effects);
        let mut config = PlayConfig::new(engine);
        config.start = StartFrom::CurrentPosition;
        config.color = match mover {
            Color::White => ColorChoice::White,
            Color::Black => ColorChoice::Black,
        };
        self.play_message(PlayMessage::Start(Box::new(config)), effects);
    }

    /// The session ended and the game stays in the editor (D2140): the
    /// parked game is let go, the problem is a game like any other now.
    fn tactics_release(&mut self) {
        self.flow.tactics.parked = None;
        self.flow.tactics.generation = None;
        self.flow.tactics.base = None;
        self.session.mark_saved();
    }

    /// Puts the parked game back, if the editor still holds the problem.
    fn tactics_restore(&mut self) {
        let parked = self.flow.tactics.parked.take();
        let ours = self.tactics_editor_is_ours();
        self.flow.tactics.generation = None;
        self.flow.tactics.base = None;
        self.restore_parked(parked, ours);
    }

    /// Puts a parked game back in the editor (D2159). `ours` says whether
    /// the editor still holds the game of the training that parked it: if
    /// not, a clean parked game is let go, one with unsaved changes never is
    /// (it comes back if the editor has nothing to lose, else it waits in
    /// the slot, else it wins over the editor). Shared with the calculation
    /// exercise (D2320).
    pub(crate) fn restore_parked(&mut self, parked: Option<Parked>, ours: bool) {
        let Some(parked) = parked else {
            return;
        };
        if !ours {
            // The editor was taken by something else. A parked game that is
            // clean is let go (D1929); one with unsaved changes never is
            // (D2159): it comes back if the editor has nothing to lose,
            // else it waits in the slot, else it wins over the editor.
            if !parked.at_stake() {
                return;
            }
            if self.needs_confirmation() {
                let stash = crate::bases::Parked {
                    game: parked.game.clone(),
                    origin: parked.origin.clone(),
                    cursor: parked.cursor,
                    dirty: true,
                };
                let current = self.bases.current();
                if let Some(slot) = self.bases.slot_mut(current)
                    && slot.parked.is_none()
                {
                    slot.parked = Some(stash);
                    return;
                }
            }
        }
        self.session.replace_game(parked.game);
        if parked.dirty {
            self.session.mark_altered();
        }
        drop(
            self.session
                .update(GameMessage::Navigate(Nav::GoToNode(parked.cursor))),
        );
        self.origin = parked.origin;
        self.collection = parked.collection;
        self.board_flipped = parked.flipped;
    }

    /// The moves of the user while a problem is on (D1930). Returns whether
    /// the message was consumed.
    pub(crate) fn tactics_intercept(
        &mut self,
        message: &GameMessage,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if !self.flow.tactics.machine.locks_editor() || !self.tactics_editor_is_ours() {
            return false;
        }
        match message {
            GameMessage::PlayMove { .. }
            | GameMessage::PlaySquares { .. }
            | GameMessage::PlayNullMove
            | GameMessage::AnswerPromotion(_)
            | GameMessage::MoveEntryChar(_)
            | GameMessage::MoveEntryTyped(_)
            | GameMessage::MoveEntryCommit
            | GameMessage::EnterMoveText(_) => {}
            // Harmless: the session handles them.
            GameMessage::AutoplayTick { .. }
            | GameMessage::StopAutoplay
            | GameMessage::DismissPrompt
            | GameMessage::CloseVariationChooser
            | GameMessage::ExitTrial
            | GameMessage::MoveEntryClear
            | GameMessage::MoveEntryBackspace
            | GameMessage::ApplySettings(_) => return false,
            _ => {
                self.flow.tactics.machine.set_notice(TacticsNotice::Locked);
                return true;
            }
        }
        if !self.flow.tactics.machine.takes_moves() {
            // The solver is still judging the previous move.
            self.flow.tactics.machine.set_notice(TacticsNotice::Locked);
            return true;
        }
        let (session_effects, played) = self.session.update_capturing(message.clone());
        effects.extend(session_effects);
        if let Some(mv) = played {
            let outs = self.flow.tactics.machine.user_move(mv);
            self.tactics_run(outs, effects);
        }
        true
    }

    /// After every message: the session ends when the editor was taken over
    /// or another mode started (D1929, D1931).
    pub(crate) fn tactics_guard(&mut self, effects: &mut Vec<Effect>) {
        self.opening_guard(effects);
        effects.append(&mut self.flow.tactics.deferred);
        if !self.flow.tactics.machine.is_live() {
            return;
        }
        // The database of a session of the current base was closed or
        // changed: the numbers of the games are stale (D1953). Checked
        // first, so the user is told why even when the same change also
        // replaced the game of the editor.
        let current = self.flow.tactics.base.as_ref().map(|b| {
            let slot = self.bases.slot(b.slot);
            (
                slot.is_some_and(|s| s.total() == b.total),
                slot.map(|s| s.source.clone()),
            )
        });
        if let Some((same_games, source)) = current {
            if !same_games {
                let outs = self
                    .flow
                    .tactics
                    .machine
                    .abort(TacticsNotice::SourceChanged);
                self.tactics_run(outs, effects);
                return;
            }
            // Same games, another handle: a change of flags reopens the
            // file. Follow it, so the old handle is let go (D1953).
            if let (Some(source), Some(base)) = (source, self.flow.tactics.base.as_mut())
                && source != base.source
            {
                base.source = source.clone();
                self.flow.tactics.machine.rebase(source);
            }
        }
        let taken = self.flow.tactics.generation.is_some() && !self.tactics_editor_is_ours();
        if taken || self.tactics_blocked() {
            self.tactics_stop(effects);
            return;
        }
        // Any engine that shows up while a problem is on is stopped: it
        // would show the answer or add its lines to the game (D1931).
        self.tactics_stop_engines(effects);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::EngineDraft;
    use crate::online::{OnlinePhase, PlayingState};

    fn parked(game: Game, dirty: bool) -> Parked {
        Parked {
            cursor: game.root(),
            game,
            origin: GameOrigin::New,
            flipped: false,
            dirty,
            collection: None,
        }
    }

    #[test]
    fn a_start_in_trial_mode_is_refused_and_the_backup_is_kept() {
        let mut state = AppState::default();
        drop(
            state
                .session
                .update(GameMessage::EnterTrial { null_move: false }),
        );
        assert!(state.session.in_trial());
        let mut effects = Vec::new();
        state.tactics_start(TacticsConfig::default(), 1, &mut effects);
        assert!(!state.tactics_live());
        assert_eq!(
            state.flow.tactics.machine.notice(),
            Some(TacticsNotice::Busy)
        );
        assert!(state.session.in_trial(), "the trial was not destroyed");
        // The start after the question about the game is refused too.
        state.tactics_start_confirmed(TacticsConfig::default(), 1, &mut effects);
        assert!(!state.tactics_live());
        assert!(state.session.in_trial());
    }

    #[test]
    fn only_a_game_with_unsaved_content_is_at_stake() {
        assert!(
            !parked(Game::new(), true).at_stake(),
            "empty and default tags"
        );
        assert!(!parked(Game::new(), false).at_stake());
        let mut with_tag = Game::new();
        with_tag.tags_mut().event = "x".to_owned();
        assert!(parked(with_tag, true).at_stake());
        let mut with_move = Game::new();
        let mv = Position::new().parse_uci("e2e4").expect("uci");
        let root = with_move.root();
        with_move.add_move(root, mv).expect("legal");
        assert!(parked(with_move.clone(), true).at_stake());
        assert!(
            !parked(with_move, false).at_stake(),
            "saved: nothing to lose"
        );
    }

    #[test]
    fn the_engine_of_the_second_opinion_is_not_chosen_while_the_help_is_locked() {
        let mut state = AppState::default();
        state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("engine");
        let config = TacticsConfig {
            second_opinion: true,
            ..TacticsConfig::default()
        };
        assert!(state.tactics_opinion_engine(&config).is_some());
        state.online.phase = OnlinePhase::Playing(PlayingState::Live);
        assert!(state.assist_locked());
        assert!(state.tactics_opinion_engine(&config).is_none());
        let off = TacticsConfig::default();
        state.online.phase = OnlinePhase::Disconnected;
        assert!(
            state.tactics_opinion_engine(&off).is_none(),
            "the option is off"
        );
    }

    fn with_move() -> Game {
        let mut game = Game::new();
        let mv = Position::new().parse_uci("e2e4").expect("uci");
        let root = game.root();
        game.add_move(root, mv).expect("legal");
        game
    }

    #[test]
    fn a_dirty_game_is_never_dropped_when_the_editor_was_taken_by_something_else() {
        // The editor holds another, clean game now: the parked one comes back.
        let mut state = AppState::default();
        state.flow.tactics.parked = Some(parked(with_move(), true));
        state.flow.tactics.generation = Some(u64::MAX);
        state.tactics_restore();
        assert_eq!(state.session.game().mainline().count(), 1);
        assert!(state.session.is_dirty());
        // A clean parked game is let go, as before.
        let mut state = AppState::default();
        state.flow.tactics.parked = Some(parked(with_move(), false));
        state.flow.tactics.generation = Some(u64::MAX);
        state.tactics_restore();
        assert_eq!(state.session.game().mainline().count(), 0);
    }

    #[test]
    fn a_dirty_game_waits_in_the_slot_when_the_editor_has_unsaved_work_too() {
        let mut state = AppState::default();
        // The editor has unsaved work of its own.
        let mv = state.session.position().parse_uci("d2d4").expect("uci");
        drop(state.session.update(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
        assert!(state.needs_confirmation());
        state.flow.tactics.parked = Some(parked(with_move(), true));
        state.flow.tactics.generation = Some(u64::MAX);
        state.tactics_restore();
        assert!(state.session.is_dirty(), "the editor was left alone");
        let current = state.bases.current();
        let slot = state.bases.slot(current).expect("slot");
        assert!(
            slot.parked.as_ref().is_some_and(|p| p.dirty),
            "kept in the slot"
        );
    }
}
