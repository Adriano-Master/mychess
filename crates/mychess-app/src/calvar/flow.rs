//! The calculation exercise in the application state (spec M8-6, phase b).
//!
//! The rules are in [`machine`](super::machine) (pure). This is the glue: it
//! hands the machine what it needs from the application (the position of
//! the editor, the engine list, the preferences path), runs what the
//! machine asks for (effects, the dedicated game of the editor, saving the
//! store, adding variants), keeps the game of the user parked while the
//! exercise is on (D2320), turns the moves of the user into entry (D2323)
//! and enforces the exclusion of the other modes (D2322). Nothing here
//! blocks (D2332).

use mychess_core::{Game, Move, Position};

use super::config::{CalvarConfig, PositionSource};
use super::engine::CalvarEngine;
use super::machine::{Calvar, CalvarOrigin, CalvarPhase, CalvarView, Out};
use super::message::{CalvarEffect, CalvarMessage, CalvarNotice, CalvarStartRequest};
use super::pick::game_position;
use super::store::{LoadedStore, store_path};
use crate::bases::{BaseError, SlotId};
use crate::effect::Effect;
use crate::engines::{EngineId, Hotkey, Protocol};
use crate::error::{FileError, RequestId};
use crate::files::GameOrigin;
use crate::flow::Pending;
use crate::session::{EngineLineToAdd, GameMessage, Nav};
use crate::shared::SharedGame;
use crate::state::AppState;
use crate::tactics::Parked;

/// What the glue remembers between messages.
#[derive(Debug, Default)]
pub(crate) struct CalvarFlow {
    pub(crate) machine: Calvar,
    parked: Option<Parked>,
    /// Effects of an exercise that ended in the middle of another message:
    /// they leave with the effects of that message.
    deferred: Vec<Effect>,
    /// The generation of the editor game that holds the exercise: any other
    /// generation means the user put something else in the editor.
    generation: Option<u64>,
    save_in_flight: bool,
    resave: bool,
    pub(crate) quit_wait: bool,
    /// A write that waits for the file to be read first (D2354).
    pending_save: bool,
    /// A start that waits for the file to be read first (D2354).
    pending_start: Option<CalvarStartRequest>,
    /// The tick that began the wait of `pending_start` (its watchdog,
    /// D2375).
    pending_since: Option<std::time::Instant>,
    /// "Clear history" that waits for the file to be read first.
    pending_reset: bool,
}

impl AppState {
    /// A picture of the exercise for the front-end.
    pub fn calvar_view(&self) -> CalvarView {
        self.flow.calvar.machine.view()
    }

    /// Whether the configuration dialog of the exercise is open.
    pub fn calvar_dialog_open(&self) -> bool {
        self.flow.calvar.machine.dialog_open()
    }

    /// Whether an exercise is on.
    pub fn calvar_live(&self) -> bool {
        self.flow.calvar.machine.is_live()
    }

    /// Whether the front-end must send [`CalvarMessage::Tick`].
    pub fn calvar_wants_ticks(&self) -> bool {
        self.flow.calvar.machine.phase().wants_ticks()
            || self.flow.calvar.machine.waiting_for_game()
            || self.flow.calvar.pending_start.is_some()
    }

    /// Whether the exercise on the screen is the review of a past one
    /// (M8-6d, D2369).
    pub fn calvar_is_review(&self) -> bool {
        self.flow.calvar.machine.is_review()
    }

    /// Whether the tactics training has a problem that the exercise could
    /// start from, and nothing forbids it (M8-6d, D2372).
    pub fn calvar_can_start_from_tactics(&self) -> bool {
        self.tactics_problem_position().is_some()
            && self.calvar_has_engine()
            && !self.modal_open()
            && !self.calvar_live()
            && !self.calvar_blocked_by_others()
            && !self.session.in_trial()
    }

    /// The phase of the exercise.
    pub fn calvar_phase(&self) -> CalvarPhase {
        self.flow.calvar.machine.phase()
    }

    /// The lines that "Add to the game" would add (D2324): those whose first
    /// move is not a continuation of the game already, so that the result
    /// never extends the main line of the user (the tree would merge an
    /// equal first move and the rest of the line would become the end of
    /// the game). Empty unless the game of the user is parked.
    fn calvar_addable(&self) -> Vec<EngineLineToAdd> {
        let Some(parked) = self.flow.calvar.parked.as_ref() else {
            return Vec::new();
        };
        Self::calvar_new_lines(
            &parked.game,
            parked.cursor,
            self.flow.calvar.machine.variants_now(),
        )
    }

    fn calvar_new_lines(
        game: &Game,
        node: mychess_core::NodeId,
        lines: Vec<EngineLineToAdd>,
    ) -> Vec<EngineLineToAdd> {
        lines
            .into_iter()
            .filter(|line| {
                line.pv
                    .first()
                    .is_some_and(|mv| matches!(game.find_child(node, *mv), Ok(None)))
            })
            .collect()
    }

    /// Whether "Add to the game" works (D2324): the result is on the screen,
    /// the position came from the editor, the node it came from has
    /// continuations (at the end of the main line the session refuses to
    /// add variants, like the Scid does for engine lines; an empty game
    /// takes the first line as its main line) and at least one line is new
    /// there.
    pub fn calvar_can_add_to_game(&self) -> bool {
        self.flow.calvar.machine.can_add_to_game()
            && self.flow.calvar.parked.as_ref().is_some_and(|p| {
                p.game.node_count() == 1
                    || p.game
                        .children(p.cursor)
                        .is_ok_and(|children| !children.is_empty())
            })
            && !self.calvar_addable().is_empty()
    }

    /// Whether the editor holds the exercise and nothing but the moves of
    /// the user may change it (D2323).
    pub(crate) fn calvar_editor_locked(&self) -> bool {
        self.flow.calvar.machine.locks_editor() && self.calvar_editor_is_ours()
    }

    /// Whether another mode owns the board or the engines (D2322).
    pub fn calvar_blocked(&self) -> bool {
        self.calvar_blocked_by_others() || self.tactics_live()
    }

    /// Like [`AppState::calvar_blocked`] without the tactics training, which
    /// may hand its problem over (M8-6d, D2372).
    fn calvar_blocked_by_others(&self) -> bool {
        self.play_live()
            || self.tournament_live()
            || self.annotation_running()
            || self.training_playing()
            || self.report.review.is_some()
            || self.assist_locked()
            || self.opening_live()
    }

    /// Whether a start is refused: another mode owns the board, or the
    /// editor is in trial mode, whose backup the exercise would destroy by
    /// replacing the game.
    fn calvar_start_refused(&self) -> bool {
        self.calvar_blocked() || self.session.in_trial()
    }

    /// Like [`AppState::calvar_start_refused`] for a request: the one that
    /// comes from the tactics training does not count the training itself,
    /// which ends only once the start is accepted (D2376).
    fn calvar_request_refused(&self, request: &CalvarStartRequest) -> bool {
        if request.origin == Some(CalvarOrigin::Tactic) {
            self.calvar_blocked_by_others() || self.session.in_trial()
        } else {
            self.calvar_start_refused()
        }
    }

    /// Whether some UCI engine can run the exercise (D2307, D2375).
    fn calvar_has_engine(&self) -> bool {
        !self.assist_locked()
            && self
                .engines
                .engines()
                .iter()
                .any(|e| e.protocol == Protocol::Uci)
    }

    /// The UCI engines of the list that the dialog offers: `(id, name)`
    /// (D2307, D2326).
    pub fn calvar_engine_choices(&self) -> Vec<(u64, String)> {
        self.engines
            .engines()
            .iter()
            .filter(|e| e.protocol == Protocol::Uci)
            .map(|e| (e.id.0, e.name.clone()))
            .collect()
    }

    fn calvar_editor_is_ours(&self) -> bool {
        self.flow.calvar.generation == Some(self.session.generation())
    }

    pub(crate) fn calvar_message(&mut self, message: CalvarMessage, effects: &mut Vec<Effect>) {
        match message {
            // A session that is on keeps the panel, not the dialog (D2325).
            CalvarMessage::Open => {
                if !self.flow.calvar.machine.is_live() {
                    self.flow.calvar.machine.open_dialog();
                    self.calvar_load_store(effects);
                }
            }
            CalvarMessage::CloseDialog => {
                // What waited for the file is given up with the dialog.
                self.calvar_forget_pending();
                self.flow.calvar.machine.close_dialog();
            }
            CalvarMessage::Start { config, now } => {
                // A read of a game that is late must not start another
                // exercise over this one (D2375).
                self.flow.calvar.machine.forget_game_wait();
                self.calvar_start(CalvarStartRequest::new(*config, now), effects);
            }
            CalvarMessage::StartFromGame { slot, number, now } => {
                self.calvar_start_game(slot, number, now, effects);
            }
            CalvarMessage::StartFromTactics { now } => self.calvar_start_tactics(now, effects),
            CalvarMessage::Review(index) => {
                let mut request = CalvarStartRequest::with_saved_config(0);
                request.review = Some(index);
                self.calvar_start(request, effects);
            }
            CalvarMessage::GameRead { request, result } => {
                self.calvar_game_read(request, result, effects);
            }
            CalvarMessage::EnterText(text) => {
                let outs = self.flow.calvar.machine.enter_text(&text);
                self.calvar_run(outs, effects);
            }
            CalvarMessage::Undo => {
                let outs = self.flow.calvar.machine.undo();
                self.calvar_run(outs, effects);
            }
            CalvarMessage::EndLine(assessment) => {
                let outs = self.flow.calvar.machine.end_line(assessment);
                self.calvar_run(outs, effects);
            }
            CalvarMessage::RemoveLine(index) => {
                let outs = self.flow.calvar.machine.remove_line(index);
                self.calvar_run(outs, effects);
            }
            CalvarMessage::Done => {
                let outs = self.flow.calvar.machine.done();
                self.calvar_run(outs, effects);
            }
            CalvarMessage::Close => {
                self.calvar_forget_pending();
                self.calvar_stop(effects);
            }
            CalvarMessage::AddToGame => {
                if !self.calvar_can_add_to_game() {
                    if self.flow.calvar.machine.can_add_to_game() {
                        self.flow.calvar.machine.set_notice(CalvarNotice::CannotAdd);
                    }
                    return;
                }
                let outs = self.flow.calvar.machine.add_to_game();
                self.calvar_run(outs, effects);
            }
            CalvarMessage::ResetStats => self.calvar_reset(effects),
            CalvarMessage::Tick(now) => {
                self.calvar_pending_watchdog(now, effects);
                let outs = self.flow.calvar.machine.tick(now);
                self.calvar_run(outs, effects);
            }
            CalvarMessage::StoreLoaded(loaded) => self.calvar_store_loaded(*loaded, effects),
            CalvarMessage::StoreSaved(result) => self.calvar_store_saved(&result, effects),
            CalvarMessage::RootAnalyzed { request, result } => {
                let outs = self.flow.calvar.machine.root_analyzed(request, result);
                self.calvar_run(outs, effects);
            }
            CalvarMessage::LinesAnalyzed { request, result } => {
                let outs = self.flow.calvar.machine.lines_analyzed(request, result);
                self.calvar_run(outs, effects);
            }
        }
    }

    /// "Clear history": waits for the file to be read first (D2354).
    fn calvar_reset(&mut self, effects: &mut Vec<Effect>) {
        if !self.calvar_store_settled() {
            self.calvar_load_store(effects);
            self.flow.calvar.pending_reset = true;
            return;
        }
        let outs = self.flow.calvar.machine.reset_stats();
        self.calvar_run(outs, effects);
        // The reset holds in memory but cannot be written: say so.
        if self.flow.calvar.machine.store_read_only() {
            self.flow
                .calvar
                .machine
                .set_notice(CalvarNotice::StoreNotSaved);
        }
    }

    /// A write of the store finished (D2318).
    fn calvar_store_saved(&mut self, result: &Result<(), FileError>, effects: &mut Vec<Effect>) {
        self.flow.calvar.machine.store_saved(result);
        self.flow.calvar.save_in_flight = false;
        if std::mem::take(&mut self.flow.calvar.resave) {
            self.calvar_save(effects);
        }
        // The program was waiting for this write to exit (D2318).
        if self.flow.calvar.quit_wait && !self.flow.calvar.save_in_flight {
            self.flow.calvar.quit_wait = false;
            self.finish_quit(effects);
        }
    }

    /// The file was read: what waited for it goes on, in this order
    /// (D2354).
    fn calvar_store_loaded(&mut self, loaded: LoadedStore, effects: &mut Vec<Effect>) {
        self.flow.calvar.machine.store_loaded(loaded);
        // What waited for the file goes on, in this order (D2354).
        if std::mem::take(&mut self.flow.calvar.pending_reset) {
            self.calvar_message(CalvarMessage::ResetStats, effects);
        }
        self.flow.calvar.pending_since = None;
        if let Some(request) = self.flow.calvar.pending_start.take() {
            self.calvar_start(request, effects);
        }
        // A write that waited for the file, unless the steps above
        // already sent the whole store out.
        if std::mem::take(&mut self.flow.calvar.pending_save) && !self.flow.calvar.save_in_flight {
            self.calvar_save(effects);
        }
        // The exit waited for a write that did not come out (the
        // file cannot be written): nothing more to wait for.
        if self.flow.calvar.quit_wait && !self.flow.calvar.save_in_flight {
            self.flow.calvar.quit_wait = false;
            self.finish_quit(effects);
        }
    }

    /// Whether the exit must wait for the history being written; asks for a
    /// deadline of five seconds the first time (D2318).
    pub(crate) fn calvar_quit_wait(&mut self, effects: &mut Vec<Effect>) -> bool {
        if !self.flow.calvar.save_in_flight && !self.flow.calvar.pending_save {
            return false;
        }
        if !self.flow.calvar.quit_wait {
            self.flow.calvar.quit_wait = true;
            effects.push(Effect::QuitDeadline {
                delay: std::time::Duration::from_secs(5),
            });
        }
        true
    }

    /// Whether the quit waits for the write of the calvar trainer.
    pub(crate) fn calvar_quit_waiting(&self) -> bool {
        self.flow.calvar.quit_wait
    }

    /// The deadline of [`AppState::calvar_quit_wait`] ran out: the write is
    /// given up. Returns whether the exit was waiting.
    pub(crate) fn calvar_quit_deadline(&mut self) -> bool {
        if !std::mem::take(&mut self.flow.calvar.quit_wait) {
            return false;
        }
        self.flow.calvar.save_in_flight = false;
        self.flow.calvar.pending_save = false;
        true
    }

    /// Gives up the start and the "clear history" that wait for the file.
    fn calvar_forget_pending(&mut self) {
        self.flow.calvar.pending_start = None;
        self.flow.calvar.pending_since = None;
        self.flow.calvar.pending_reset = false;
        self.flow.calvar.machine.forget_game_wait();
    }

    /// The start that waits for the file gives up after the same time as the
    /// read of a game (D2371, D2375): the dialog opens with the notice.
    fn calvar_pending_watchdog(&mut self, now: std::time::Instant, effects: &mut Vec<Effect>) {
        if self.flow.calvar.pending_start.is_none() {
            self.flow.calvar.pending_since = None;
            return;
        }
        let since = *self.flow.calvar.pending_since.get_or_insert(now);
        if now.saturating_duration_since(since) < std::time::Duration::from_secs(20) {
            return;
        }
        self.flow.calvar.pending_since = None;
        if let Some(request) = self.flow.calvar.pending_start.take() {
            let external = request.root.is_some() || request.review.is_some();
            self.calvar_fail(CalvarNotice::TimedOut, external, effects);
        }
    }

    /// Whether the file was read, or there is no file to read.
    fn calvar_store_settled(&self) -> bool {
        self.flow.calvar.machine.store_ready() || self.flow.prefs_path.is_none()
    }

    /// Reads `calvar.toml` once (D2317).
    fn calvar_load_store(&mut self, effects: &mut Vec<Effect>) {
        if self.flow.calvar.machine.store_requested() {
            return;
        }
        let Some(prefs) = self.flow.prefs_path.as_deref() else {
            return;
        };
        let path = store_path(prefs);
        self.flow.calvar.machine.begin_load();
        effects.push(Effect::Calvar(CalvarEffect::LoadStore { path }));
    }

    fn calvar_start(&mut self, request: CalvarStartRequest, effects: &mut Vec<Effect>) {
        // Never over an exercise that is on: it would lose the parked game.
        if self.flow.calvar.machine.is_live() {
            return;
        }
        let external = request.root.is_some() || request.review.is_some();
        self.calvar_load_store(effects);
        // The file is read before anything is recorded, or the write that
        // follows would erase it (D2354).
        if !self.calvar_store_settled() {
            self.flow.calvar.pending_start = Some(request);
            return;
        }
        if self.calvar_request_refused(&request) || self.modal_open() {
            self.calvar_fail(CalvarNotice::Busy, external, effects);
            return;
        }
        // The game of the editor is parked, not saved: the user is asked
        // first whether to save it (D2320). "Don't save" parks it with its
        // changes, which come back marked as unsaved: nothing is lost.
        if !request.confirmed && !self.calvar_editor_is_ours() && self.needs_confirmation() {
            self.proceed_or_ask(Pending::CalvarStart(Box::new(request)), effects);
            return;
        }
        self.calvar_begin(request, effects);
    }

    /// Shows why a start did not happen. A start that came from outside the
    /// dialog (`external`) has no panel or dialog to show it in, so the
    /// dialog opens with the notice (D2371).
    fn calvar_fail(&mut self, notice: CalvarNotice, external: bool, effects: &mut Vec<Effect>) {
        let machine = &mut self.flow.calvar.machine;
        if external && !machine.is_live() && !machine.dialog_open() && !self.modal_open() {
            self.flow.calvar.machine.open_dialog();
            self.calvar_load_store(effects);
        }
        self.flow.calvar.machine.set_notice(notice);
    }

    /// "Calculate variants" on a game of the list (M8-6d, D2368): the game is
    /// read by an effect and its position becomes the problem.
    fn calvar_start_game(
        &mut self,
        slot: SlotId,
        number: u32,
        now: u64,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.calvar.machine.is_live() {
            self.flow.calvar.machine.set_notice(CalvarNotice::Busy);
            return;
        }
        if self.calvar_start_refused() || self.modal_open() || self.list_job().is_some() {
            self.calvar_fail(CalvarNotice::Busy, true, effects);
            return;
        }
        let Some((raw, source)) = self
            .bases
            .slot(slot)
            .filter(|s| number >= 1 && number <= s.total())
            .map(|s| (s.filter.get(number), s.source.clone()))
        else {
            self.calvar_fail(CalvarNotice::GameUnreadable, true, effects);
            return;
        };
        // The file is read meanwhile: the configuration is the saved one
        // (D2375).
        self.calvar_load_store(effects);
        let request = self.flow.calvar.machine.expect_game(number, raw, now);
        effects.push(Effect::Calvar(CalvarEffect::ReadGame {
            request,
            number,
            source,
        }));
    }

    /// The answer of the read of a game for a start (D2371): an old one was
    /// dropped by the machine.
    fn calvar_game_read(
        &mut self,
        request: RequestId,
        result: Result<SharedGame, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(wait) = self.flow.calvar.machine.take_game_wait(request) else {
            return;
        };
        let Ok(game) = result else {
            self.calvar_fail(CalvarNotice::GameUnreadable, true, effects);
            return;
        };
        let root = game_position(&game, wait.raw_filter);
        let mut start = CalvarStartRequest::with_saved_config(wait.now);
        start.root = Some(Box::new(root));
        start.origin = Some(CalvarOrigin::Game(wait.number));
        self.calvar_start(start, effects);
    }

    /// "Calculate variants" on the problem of the tactics training (M8-6d,
    /// D2372): the training ends, giving the game of the user back, and the
    /// exercise starts from the problem. The question about the game was
    /// answered when the training started.
    fn calvar_start_tactics(&mut self, now: u64, effects: &mut Vec<Effect>) {
        if self.flow.calvar.machine.is_live() {
            self.flow.calvar.machine.set_notice(CalvarNotice::Busy);
            return;
        }
        let Some(position) = self.tactics_problem_position() else {
            self.calvar_fail(CalvarNotice::NoProblem, true, effects);
            return;
        };
        if self.calvar_blocked_by_others() || self.session.in_trial() || self.modal_open() {
            self.calvar_fail(CalvarNotice::Busy, true, effects);
            return;
        }
        // Everything that could refuse is asked before the training ends
        // (the checks above and the engine): a refused start loses nothing
        // (D2375).
        if !self.calvar_has_engine() {
            self.calvar_fail(CalvarNotice::NoEngine, true, effects);
            return;
        }
        // The training ends only when the start is accepted, in
        // `calvar_begin` (D2376).
        self.calvar_load_store(effects);
        let mut request = CalvarStartRequest::with_saved_config(now);
        request.root = Some(Box::new(position));
        request.origin = Some(CalvarOrigin::Tactic);
        request.confirmed = true;
        self.calvar_start(request, effects);
    }

    /// The start after the question about the game was answered (D2320).
    pub(crate) fn calvar_start_confirmed(
        &mut self,
        request: CalvarStartRequest,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.calvar.machine.is_live() {
            return;
        }
        let external = request.root.is_some() || request.review.is_some();
        if self.calvar_request_refused(&request) {
            self.calvar_fail(CalvarNotice::Busy, external, effects);
            return;
        }
        self.calvar_begin(request, effects);
    }

    /// The engine of the exercise (D2307): the one the dialog names if it is
    /// a UCI engine of the list, else the one of the key F2, else the first
    /// UCI engine. None while the help is locked.
    fn calvar_engine(&self, config: &CalvarConfig) -> Option<CalvarEngine> {
        if self.assist_locked() {
            return None;
        }
        let list = &self.engines;
        let uci = |id: EngineId| list.get(id).filter(|entry| entry.protocol == Protocol::Uci);
        let entry = config
            .engine
            .and_then(|n| uci(EngineId(n)))
            .or_else(|| list.hotkey_owner(Hotkey::F2).and_then(uci))
            .or_else(|| list.engines().iter().find(|e| e.protocol == Protocol::Uci))?;
        Some(CalvarEngine(
            entry.to_config(&self.prefs.analysis.run_defaults()),
        ))
    }

    fn calvar_begin(&mut self, request: CalvarStartRequest, effects: &mut Vec<Effect>) {
        let external = request.root.is_some() || request.review.is_some();
        if let Some(index) = request.review {
            let outs = self.flow.calvar.machine.review(index);
            if self.flow.calvar.machine.is_live() {
                self.calvar_run(outs, effects);
            } else {
                let notice = self.flow.calvar.machine.notice();
                self.calvar_fail(notice.unwrap_or(CalvarNotice::NoReview), true, effects);
            }
            return;
        }
        let CalvarStartRequest {
            config,
            now,
            root,
            origin,
            ..
        } = request;
        // The last configuration is taken now, after the file was read.
        let mut config = config.unwrap_or_else(|| self.flow.calvar.machine.config().clone());
        let root = match root {
            Some(root) => Some(*root),
            None => match config.source {
                PositionSource::Current => Some(self.session.position().clone()),
                PositionSource::Fen => Position::from_fen(config.fen.trim()).ok(),
            },
        };
        let Some(root) = root else {
            self.calvar_fail(CalvarNotice::BadFen, external, effects);
            return;
        };
        let Some(engine) = self.calvar_engine(&config) else {
            self.calvar_fail(CalvarNotice::NoEngine, external, effects);
            return;
        };
        let origin = origin.unwrap_or(match config.source {
            PositionSource::Current => CalvarOrigin::Editor,
            PositionSource::Fen => CalvarOrigin::Fen,
        });
        // Everything that can refuse is asked before the training of tactics
        // ends, so a refused start loses nothing (D2376).
        if origin == CalvarOrigin::Tactic {
            if root.legal_moves().is_empty() {
                self.calvar_fail(CalvarNotice::NoMoves, external, effects);
                return;
            }
            self.tactics_stop(effects);
        }
        if external {
            // The position is not the one of the dialog: what the dialog
            // saved (source and FEN) is not changed by it (D2368).
            let saved = self.flow.calvar.machine.config();
            config.source = saved.source;
            config.fen.clone_from(&saved.fen);
        }
        let outs = self
            .flow
            .calvar
            .machine
            .start_from(config, root, engine, now, origin);
        if self.flow.calvar.machine.is_live() {
            self.tactics_stop_engines(effects);
        } else if external {
            let notice = self.flow.calvar.machine.notice();
            self.calvar_fail(notice.unwrap_or(CalvarNotice::NoMoves), true, effects);
        }
        self.calvar_run(outs, effects);
    }

    /// Ends the exercise in the middle of another message, putting the game
    /// of the user back at once; the effects leave with that message
    /// (D2321).
    pub(crate) fn calvar_halt(&mut self) {
        if !self.flow.calvar.machine.is_live() {
            return;
        }
        let mut effects = Vec::new();
        self.calvar_stop(&mut effects);
        self.flow.calvar.deferred.extend(effects);
    }

    /// Ends the exercise and puts the game of the user back.
    pub(crate) fn calvar_stop(&mut self, effects: &mut Vec<Effect>) {
        let outs = self.flow.calvar.machine.close();
        self.calvar_run(outs, effects);
    }

    fn calvar_run(&mut self, outs: Vec<Out>, effects: &mut Vec<Effect>) {
        let mut save = false;
        for out in outs {
            match out {
                Out::Effect(effect) => effects.push(Effect::Calvar(effect)),
                Out::ShowBoard { root, moves } => self.calvar_show(&root, &moves),
                Out::SaveStore => save = true,
                Out::StartFailed(notice) => self.calvar_fail(notice, true, effects),
                Out::Closed => {
                    self.calvar_restore();
                }
                Out::AddToGame { root, lines } => self.calvar_add(&root, lines, effects),
            }
        }
        if save {
            self.calvar_save(effects);
        }
    }

    /// Writes the store, one write at a time so that an old one never lands
    /// after a new one.
    fn calvar_save(&mut self, effects: &mut Vec<Effect>) {
        // A file of a newer version, or one that could not be read, is not
        // written over (D2317).
        if self.flow.calvar.machine.store_read_only() {
            return;
        }
        let Some(prefs) = self.flow.prefs_path.as_deref() else {
            return;
        };
        // Never before the file was read: the write would replace it with
        // the defaults (D2354).
        if !self.flow.calvar.machine.store_ready() {
            self.flow.calvar.pending_save = true;
            return;
        }
        if self.flow.calvar.save_in_flight {
            self.flow.calvar.resave = true;
            return;
        }
        let path = store_path(prefs);
        self.flow.calvar.save_in_flight = true;
        effects.push(Effect::Calvar(CalvarEffect::SaveStore {
            path,
            store: Box::new(self.flow.calvar.machine.store().clone()),
        }));
    }

    /// Puts the dedicated game in the editor, parking the game of the user
    /// the first time (D2320): the start position followed by `moves`.
    fn calvar_show(&mut self, root: &Position, moves: &[Move]) {
        if !self.calvar_editor_is_ours() {
            self.flow.calvar.parked = Some(Parked::capture(self));
        }
        let mut game = Game::from_position(root.clone());
        "Calculation".clone_into(&mut game.tags_mut().event);
        let mut node = game.root();
        for mv in moves {
            match game.add_move(node, *mv) {
                Ok(next) => node = next,
                Err(_) => break,
            }
        }
        self.session.replace_game(game);
        self.flow.calvar.generation = Some(self.session.generation());
        drop(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
        self.session.mark_saved();
        self.origin = GameOrigin::New;
        self.collection = None;
        self.board_flipped = root.turn() == mychess_core::Color::Black;
        self.setup = None;
        self.info_dialog = None;
        self.chooser = None;
        self.discard_prompt = None;
    }

    /// Puts the parked game back, if the editor still holds the exercise.
    /// Returns whether the game of the user is back in the place it was.
    fn calvar_restore(&mut self) -> bool {
        let parked = self.flow.calvar.parked.take();
        let ours = self.calvar_editor_is_ours();
        self.flow.calvar.generation = None;
        let back = parked.is_some() && ours;
        self.restore_parked(parked, ours);
        back
    }

    /// Adds the lines to the game as variants at the node of the exercise,
    /// if the game of the user is back there (D2324).
    fn calvar_add(
        &mut self,
        root: &Position,
        lines: Vec<EngineLineToAdd>,
        effects: &mut Vec<Effect>,
    ) {
        let node = self.session.cursor();
        let lines = Self::calvar_new_lines(self.session.game(), node, lines);
        let same = self.session.position().fen() == root.fen();
        if !same || lines.is_empty() {
            self.flow.calvar.machine.set_notice(CalvarNotice::CannotAdd);
            return;
        }
        effects.extend(
            self.session
                .update(GameMessage::AddEngineLines { node, lines }),
        );
    }

    /// The moves of the user while the exercise is on (D2323). Returns
    /// whether the message was consumed.
    pub(crate) fn calvar_intercept(
        &mut self,
        message: &GameMessage,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if !self.flow.calvar.machine.is_live() || !self.calvar_editor_is_ours() {
            return false;
        }
        // The trial backup would be destroyed by the next game the exercise
        // puts in the editor, in any phase (D2378).
        if matches!(message, GameMessage::EnterTrial { .. }) {
            self.flow.calvar.machine.set_notice(CalvarNotice::Locked);
            return true;
        }
        if !self.flow.calvar.machine.locks_editor() {
            return false;
        }
        let machine = &mut self.flow.calvar.machine;
        let outs = match message {
            GameMessage::PlayMove { mv, .. } => machine.board_move(*mv),
            GameMessage::PlaySquares {
                from,
                to,
                promotion,
                ..
            } => machine.board_squares(Some(*from), Some(*to), *promotion),
            GameMessage::MoveEntryChar(c) | GameMessage::MoveEntryTyped(c) => {
                machine.entry_char(*c);
                Vec::new()
            }
            GameMessage::MoveEntryBackspace => {
                machine.entry_backspace();
                Vec::new()
            }
            GameMessage::MoveEntryClear => {
                machine.entry_clear();
                Vec::new()
            }
            GameMessage::MoveEntryCommit => machine.entry_commit(),
            GameMessage::EnterMoveText(text) => machine.enter_text(text),
            // The prompt of a promotion never opens: the board names the
            // piece and a missing one is a queen.
            GameMessage::AnswerPromotion(_) => Vec::new(),
            // Harmless: the session handles them.
            GameMessage::AutoplayTick { .. }
            | GameMessage::StopAutoplay
            | GameMessage::DismissPrompt
            | GameMessage::CloseVariationChooser
            | GameMessage::ExitTrial
            | GameMessage::ApplySettings(_) => return false,
            _ => {
                machine.set_notice(CalvarNotice::Locked);
                return true;
            }
        };
        self.calvar_run(outs, effects);
        true
    }

    /// After every message: the exercise ends when the editor was taken
    /// over or another mode started (D2321, D2322).
    pub(crate) fn calvar_guard(&mut self, effects: &mut Vec<Effect>) {
        effects.append(&mut self.flow.calvar.deferred);
        if !self.flow.calvar.machine.is_live() {
            return;
        }
        let taken = self.flow.calvar.generation.is_some() && !self.calvar_editor_is_ours();
        if taken || self.calvar_blocked() {
            self.calvar_stop(effects);
            return;
        }
        // Any engine that shows up while the exercise is on is stopped: it
        // would show the answer (D2322).
        self.tactics_stop_engines(effects);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::DiscardPrompt;

    fn start_message() -> CalvarMessage {
        CalvarMessage::Start {
            config: Box::default(),
            now: 1,
        }
    }

    fn with_prefs() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("dir");
        let state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
        (state, dir)
    }

    fn saves(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|e| matches!(e, Effect::Calvar(CalvarEffect::SaveStore { .. })))
            .count()
    }

    #[tokio::test]
    async fn a_tactics_start_on_a_position_without_moves_keeps_the_training() {
        use crate::tactics::{TacticsConfig, TacticsMessage, TacticsPhase};
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("p.pgn");
        std::fs::write(
            &file,
            "[Event \"p\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\"]\n\n*\n\n",
        )
        .expect("write");
        let mut state = AppState::default();
        state
            .engines
            .add(crate::engines::EngineDraft::new("Fake", "/fake"))
            .expect("engine");
        let mut queue: std::collections::VecDeque<Effect> = crate::update(
            &mut state,
            crate::Message::Tactics(TacticsMessage::Start {
                config: Box::new(TacticsConfig {
                    file: Some(file),
                    ..TacticsConfig::default()
                }),
                seed: 1,
            }),
        )
        .into();
        while let Some(effect) = queue.pop_front() {
            if matches!(effect, Effect::Tactics(_)) {
                let answer = crate::perform(effect).await;
                queue.extend(crate::update(&mut state, answer));
            }
        }
        assert_eq!(state.tactics_phase(), TacticsPhase::Solving);
        // Back-rank mate: black has no legal move.
        let mate = Position::from_fen("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1").expect("fen");
        let mut request = CalvarStartRequest::with_saved_config(1);
        request.root = Some(Box::new(mate));
        request.origin = Some(CalvarOrigin::Tactic);
        request.confirmed = true;
        let mut effects = Vec::new();
        state.calvar_begin(request, &mut effects);
        assert!(!state.calvar_live());
        assert_eq!(state.calvar_view().notice, Some(CalvarNotice::NoMoves));
        assert_eq!(state.tactics_phase(), TacticsPhase::Solving, "kept");
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
        state.calvar_message(start_message(), &mut effects);
        assert!(!state.calvar_live());
        assert_eq!(state.calvar_view().notice, Some(CalvarNotice::Busy));
        assert!(state.session.in_trial(), "the trial was not destroyed");
    }

    #[test]
    fn a_start_under_a_dialog_is_refused() {
        let mut state = AppState::default();
        state.discard_prompt = Some(DiscardPrompt {
            pending: Pending::Quit,
        });
        let mut effects = Vec::new();
        state.calvar_message(start_message(), &mut effects);
        assert!(!state.calvar_live());
        assert_eq!(state.calvar_view().notice, Some(CalvarNotice::Busy));
        assert!(state.discard_prompt.is_some(), "the dialog is not closed");
    }

    #[test]
    fn closing_the_dialog_or_the_exercise_forgets_what_waited_for_the_file() {
        for close in [CalvarMessage::CloseDialog, CalvarMessage::Close] {
            let (mut state, _dir) = with_prefs();
            let mut effects = Vec::new();
            state.calvar_message(start_message(), &mut effects);
            state.calvar_message(CalvarMessage::ResetStats, &mut effects);
            assert!(state.flow.calvar.pending_start.is_some());
            assert!(state.flow.calvar.pending_reset);
            state.calvar_message(close, &mut effects);
            assert!(state.flow.calvar.pending_start.is_none());
            assert!(!state.flow.calvar.pending_reset);
            // The file arrives: nothing starts, nothing is cleared.
            state.calvar_message(CalvarMessage::StoreLoaded(Box::default()), &mut effects);
            assert!(!state.calvar_live());
            assert_eq!(saves(&effects), 0);
        }
    }

    #[test]
    fn a_write_that_waited_for_the_file_is_not_sent_twice() {
        let (mut state, _dir) = with_prefs();
        let mut effects = Vec::new();
        state.calvar_message(CalvarMessage::ResetStats, &mut effects);
        state.flow.calvar.pending_save = true;
        effects.clear();
        state.calvar_message(CalvarMessage::StoreLoaded(Box::default()), &mut effects);
        assert_eq!(saves(&effects), 1, "{effects:?}");
        assert!(!state.flow.calvar.resave, "no second write is queued");
    }

    #[test]
    fn the_exit_waits_for_a_write_that_waits_for_the_file() {
        let (mut state, _dir) = with_prefs();
        state.flow.calvar.pending_save = true;
        let mut effects = Vec::new();
        assert!(state.calvar_quit_wait(&mut effects));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::QuitDeadline { .. }))
        );
        assert!(state.calvar_quit_deadline());
        assert!(!state.flow.calvar.pending_save);
        assert!(!state.calvar_quit_wait(&mut Vec::new()));
    }

    #[test]
    fn an_edit_from_outside_does_not_change_the_game_while_the_exercise_is_on() {
        let mut state = AppState::default();
        state
            .engines
            .add(crate::engines::EngineDraft::new("Fake", "/fake"))
            .expect("engine");
        let mut effects = Vec::new();
        state.calvar_message(start_message(), &mut effects);
        assert!(state.calvar_live());
        assert!(state.calvar_editor_locked());
        let before = state.session.game().node_count();
        let mv = state.session.position().parse_uci("e2e4").expect("uci");
        state.apply_game(
            GameMessage::PlayMove {
                mv,
                options: crate::session::PlayOptions::default(),
            },
            &mut effects,
        );
        assert_eq!(state.session.game().node_count(), before);
    }
}
