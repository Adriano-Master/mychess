//! The opening trainer in the application state (spec M8-5, phase b).
//!
//! The rules are in [`machine`](super::machine) (pure). This is the glue: it
//! resolves the source of the repertoire from the open databases, runs what
//! the machine asks for (effects, the lines in the editor, saving the
//! store), keeps the game of the user parked while a line is on (D2229),
//! turns the moves of the user into judgements (D2230) and enforces the
//! exclusion of the other modes (D2232). Nothing here blocks (D2235).

use std::path::Path;
use std::sync::Arc;

use mychess_core::{Color, Game, Move};
use mychess_db::GameSource;

use super::build::{BuildSource, GamesSource, MAX_GAMES};
use super::config::{OpeningConfig, RepSourceKind, TrainSide};
use super::machine::{OpeningPhase, OpeningTrainer, OpeningView, Out};
use super::message::{OpeningEffect, OpeningMessage, OpeningNotice, OpeningStartRequest};
use super::store::store_path;
use crate::bases::{BaseKind, SlotId};
use crate::effect::Effect;
use crate::files::GameOrigin;
use crate::flow::Pending;
use crate::session::{GameMessage, Nav, PlayOptions};
use crate::state::AppState;
use crate::tactics::{Parked, SourceHandle, source_key};

/// The database type "Openings for White" (`docs/formatos/si4.md`, 6).
pub const TYPE_OPENINGS_WHITE: u32 = 17;
/// The database type "Openings for Black".
pub const TYPE_OPENINGS_BLACK: u32 = 18;
/// The database type "Openings for either color".
pub const TYPE_OPENINGS_EITHER: u32 = 19;

/// Which of the open databases serve as repertoire (D2203): for each colour
/// asked for by `side`, the first one with the type of that colour, else the
/// first of the type "either". `slots` are the databases in the order of the
/// slots with their `base_type`. Returns `(white, black)`.
pub fn pick_typed<T: Copy>(slots: &[(T, u32)], side: TrainSide) -> (Option<T>, Option<T>) {
    let find = |kind: u32| slots.iter().find(|(_, t)| *t == kind).map(|(id, _)| *id);
    let white = find(TYPE_OPENINGS_WHITE).or_else(|| find(TYPE_OPENINGS_EITHER));
    let black = find(TYPE_OPENINGS_BLACK).or_else(|| find(TYPE_OPENINGS_EITHER));
    match side {
        TrainSide::White => (white, None),
        TrainSide::Black => (None, black),
        TrainSide::Both => (white, black),
    }
}

/// What the glue remembers between messages.
#[derive(Debug, Default)]
pub(crate) struct OpeningFlow {
    pub(crate) machine: OpeningTrainer,
    parked: Option<Parked>,
    /// The slot the parked game was taken from (D2256).
    parked_slot: Option<SlotId>,
    /// The databases that the repertoire is being read from and how many
    /// games they had (D2237); empty once the repertoire is ready.
    bases: Vec<(SlotId, u32)>,
    /// Effects of a session that ended in the middle of another message.
    deferred: Vec<Effect>,
    /// The generation of the editor game that holds the line.
    generation: Option<u64>,
    save_in_flight: bool,
    resave: bool,
    /// A write was asked for before the file was read; it goes out when the
    /// read is answered (D2255).
    save_waits_for_load: bool,
    /// The program waits for the write of the score before it exits.
    pub(crate) quit_wait: bool,
}

/// The key of a repertoire in the store: the source and the colour.
fn rep_key(base: &str, color: Color) -> String {
    if base.is_empty() {
        return String::new();
    }
    let suffix = match color {
        Color::White => 'w',
        Color::Black => 'b',
    };
    format!("{base}|{suffix}")
}

fn base_key(path: Option<&Path>) -> String {
    path.map(source_key).unwrap_or_default()
}

impl AppState {
    /// A picture of the trainer for the front-end.
    pub fn opening_view(&self) -> OpeningView {
        self.flow.opening.machine.view()
    }

    /// Whether the configuration dialog of the trainer is open.
    pub fn opening_dialog_open(&self) -> bool {
        self.flow.opening.machine.dialog_open()
    }

    /// Whether an opening-trainer session is on.
    pub fn opening_live(&self) -> bool {
        self.flow.opening.machine.is_live()
    }

    /// Whether the front-end must send [`OpeningMessage::Tick`].
    pub fn opening_wants_ticks(&self) -> bool {
        self.flow.opening.machine.phase().wants_ticks()
    }

    /// The phase of the trainer.
    pub fn opening_phase(&self) -> OpeningPhase {
        self.flow.opening.machine.phase()
    }

    /// Whether another mode owns the board or the engines (D2232).
    pub fn opening_blocked(&self) -> bool {
        self.play_live()
            || self.tournament_live()
            || self.annotation_running()
            || self.training_playing()
            || self.report.review.is_some()
            || self.assist_locked()
            || self.tactics_live()
            || self.calvar_live()
    }

    /// Whether a start is refused: another mode owns the board, or the
    /// editor is in trial mode, whose backup the trainer would destroy by
    /// replacing the game.
    fn opening_start_refused(&self) -> bool {
        self.opening_blocked() || self.session.in_trial()
    }

    /// Whether the editor holds the line of the trainer and the user is to
    /// move: nothing but the user's move may change it (D2230, D2258).
    pub(crate) fn opening_editor_locked(&self) -> bool {
        self.flow.opening.machine.locks_editor() && self.opening_editor_is_ours()
    }

    pub(crate) fn opening_editor_is_ours(&self) -> bool {
        self.flow.opening.generation == Some(self.session.generation())
    }

    pub(crate) fn opening_message(&mut self, message: OpeningMessage, effects: &mut Vec<Effect>) {
        match message {
            OpeningMessage::Open => {
                if !self.flow.opening.machine.is_live() {
                    self.flow.opening.machine.open_dialog();
                    self.opening_load_store(effects);
                }
            }
            OpeningMessage::CloseDialog => self.flow.opening.machine.close_dialog(),
            OpeningMessage::Start {
                config,
                seed,
                today,
            } => self.opening_start(*config, seed, today, effects),
            OpeningMessage::Hint => self.flow.opening.machine.hint_step(),
            OpeningMessage::Next => {
                let outs = self.flow.opening.machine.next_line();
                self.opening_run(outs, effects);
            }
            OpeningMessage::Retry => {
                let outs = self.flow.opening.machine.retry();
                self.opening_run(outs, effects);
            }
            OpeningMessage::Report => self.flow.opening.machine.toggle_report(),
            OpeningMessage::Close => self.opening_stop(effects),
            OpeningMessage::ResetStats(config) => {
                let keys = self.opening_keys(&config);
                let outs = self.flow.opening.machine.reset(&keys);
                self.opening_run(outs, effects);
                // The reset holds in memory but cannot be written: say so
                // (D2226).
                if self.flow.opening.machine.store_read_only() {
                    self.flow
                        .opening
                        .machine
                        .set_notice(OpeningNotice::StoreNotSaved);
                }
            }
            OpeningMessage::Tick(now) => {
                let outs = self.flow.opening.machine.tick(now);
                self.opening_run(outs, effects);
            }
            OpeningMessage::StoreLoaded(loaded) => {
                self.flow.opening.machine.store_loaded(*loaded);
                if std::mem::take(&mut self.flow.opening.save_waits_for_load) {
                    self.opening_save(effects);
                }
                // The exit waited for a write that did not come out (the
                // file cannot be written): nothing more to wait for.
                if self.flow.opening.quit_wait && !self.flow.opening.save_in_flight {
                    self.flow.opening.quit_wait = false;
                    self.finish_quit(effects);
                }
            }
            OpeningMessage::StoreSaved(result) => {
                self.flow.opening.machine.store_saved(&result);
                self.flow.opening.save_in_flight = false;
                if std::mem::take(&mut self.flow.opening.resave) {
                    self.opening_save(effects);
                }
                // The program was waiting for this write to exit (D2227).
                if self.flow.opening.quit_wait && !self.flow.opening.save_in_flight {
                    self.flow.opening.quit_wait = false;
                    self.finish_quit(effects);
                }
            }
            OpeningMessage::Built { request, result } => {
                // A dialog opened while the repertoire was built would be
                // closed in silence by the first line: the start gives up.
                if self.modal_open() && self.flow.opening.machine.phase() == OpeningPhase::Building
                {
                    let outs = self.flow.opening.machine.abort(OpeningNotice::Busy);
                    self.opening_run(outs, effects);
                    return;
                }
                let outs = self.flow.opening.machine.built(request, result);
                self.opening_run(outs, effects);
            }
        }
    }

    /// Whether the exit must wait for the score being written; asks for a
    /// deadline of five seconds the first time (D2227).
    pub(crate) fn opening_quit_wait(&mut self, effects: &mut Vec<Effect>) -> bool {
        if !self.flow.opening.save_in_flight && !self.flow.opening.save_waits_for_load {
            return false;
        }
        if !self.flow.opening.quit_wait {
            self.flow.opening.quit_wait = true;
            effects.push(Effect::QuitDeadline {
                delay: std::time::Duration::from_secs(5),
            });
        }
        true
    }

    /// Whether the quit waits for the write of the opening trainer.
    pub(crate) fn opening_quit_waiting(&self) -> bool {
        self.flow.opening.quit_wait
    }

    /// The deadline of [`AppState::opening_quit_wait`] ran out: the write is
    /// given up. Returns whether the exit was waiting.
    pub(crate) fn opening_quit_deadline(&mut self) -> bool {
        if !std::mem::take(&mut self.flow.opening.quit_wait) {
            return false;
        }
        self.flow.opening.save_in_flight = false;
        self.flow.opening.save_waits_for_load = false;
        true
    }

    /// Reads `opening_trainer.toml` once (D2225).
    fn opening_load_store(&mut self, effects: &mut Vec<Effect>) {
        if self.flow.opening.machine.store_requested() {
            return;
        }
        let Some(prefs) = self.flow.prefs_path.as_deref() else {
            return;
        };
        let path = store_path(prefs);
        self.flow.opening.machine.begin_load();
        effects.push(Effect::OpeningTrainer(OpeningEffect::LoadStore { path }));
    }

    /// The databases that can serve as repertoire, with their type, in the
    /// order of the slots. The Clipbase never does (D2203).
    fn opening_typed_slots(&self) -> Vec<(SlotId, u32)> {
        self.bases
            .iter()
            .filter(|(_, slot)| slot.source.read().kind() != BaseKind::Clipbase)
            .map(|(id, slot)| (id, slot.source.read().header().base_type()))
            .collect()
    }

    fn opening_games_of(&self, id: SlotId) -> Option<GamesSource> {
        let slot = self.bases.slot(id)?;
        // One more than the limit tells that the filter was cut (D2254).
        let mut numbers: Vec<u32> = slot.filter.iter_games().take(MAX_GAMES + 1).collect();
        let truncated = numbers.len() > MAX_GAMES;
        numbers.truncate(MAX_GAMES);
        if numbers.is_empty() {
            return None;
        }
        Some(GamesSource {
            handle: SourceHandle::Base {
                source: slot.source.clone(),
                numbers: Arc::new(numbers),
            },
            key: base_key(slot.path.as_deref()),
            truncated,
        })
    }

    /// The keys of the score (white, black) of the repertoire that `config`
    /// names, without any I/O.
    fn opening_keys_pair(&self, config: &OpeningConfig) -> [String; 2] {
        let keys_of = |base: &str| [rep_key(base, Color::White), rep_key(base, Color::Black)];
        match config.source {
            RepSourceKind::File => keys_of(&base_key(config.file.as_deref())),
            RepSourceKind::CurrentBase => keys_of(&base_key(
                self.bases
                    .slot(self.bases.current())
                    .and_then(|s| s.path.as_deref()),
            )),
            RepSourceKind::TypedBase => {
                let (white, black) = pick_typed(&self.opening_typed_slots(), config.side);
                let path_of = |id: Option<SlotId>| {
                    base_key(
                        id.and_then(|id| self.bases.slot(id))
                            .and_then(|s| s.path.as_deref()),
                    )
                };
                [
                    rep_key(&path_of(white), Color::White),
                    rep_key(&path_of(black), Color::Black),
                ]
            }
        }
    }

    fn opening_keys(&self, config: &OpeningConfig) -> Vec<String> {
        self.opening_keys_pair(config)
            .into_iter()
            .filter(|k| !k.is_empty())
            .collect()
    }

    /// The source of the repertoire that `config` names, with the keys of
    /// the score, or the remark that says why there is none (D2202, D2203).
    fn opening_source(
        &self,
        config: &OpeningConfig,
    ) -> Result<(BuildSource, [String; 2]), OpeningNotice> {
        let keys = self.opening_keys_pair(config);
        match config.source {
            RepSourceKind::File => {
                let path = config.file.clone().ok_or(OpeningNotice::NoSource)?;
                Ok((BuildSource::File(path), keys))
            }
            RepSourceKind::CurrentBase => {
                let games = self
                    .opening_games_of(self.bases.current())
                    .ok_or(OpeningNotice::NoSource)?;
                Ok((BuildSource::Games(games), keys))
            }
            RepSourceKind::TypedBase => {
                let (white, black) = pick_typed(&self.opening_typed_slots(), config.side);
                if white.is_none() && black.is_none() {
                    return Err(OpeningNotice::NoRepertoire);
                }
                let white = white.and_then(|id| self.opening_games_of(id));
                let black = black.and_then(|id| self.opening_games_of(id));
                if white.is_none() && black.is_none() {
                    return Err(OpeningNotice::NoRepertoire);
                }
                Ok((BuildSource::Typed { white, black }, keys))
            }
        }
    }

    /// The names of the files that the repertoire of `config` would be read
    /// from, for the dialog (D2239): empty when there is none.
    pub fn opening_source_names(&self, config: &OpeningConfig) -> Vec<String> {
        let name_of = |path: Option<&Path>| {
            path.map_or_else(
                || "-".to_owned(),
                |p| {
                    p.file_name().map_or_else(
                        || p.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    )
                },
            )
        };
        match config.source {
            RepSourceKind::File => config
                .file
                .as_deref()
                .map(|p| vec![name_of(Some(p))])
                .unwrap_or_default(),
            RepSourceKind::CurrentBase => self
                .bases
                .current_slot()
                .map(|s| vec![name_of(s.path.as_deref())])
                .unwrap_or_default(),
            RepSourceKind::TypedBase => {
                let (white, black) = pick_typed(&self.opening_typed_slots(), config.side);
                let mut names = Vec::new();
                for id in white.into_iter().chain(black) {
                    if let Some(slot) = self.bases.slot(id) {
                        let name = name_of(slot.path.as_deref());
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                }
                names
            }
        }
    }

    /// The slots that the repertoire of `config` is read from.
    fn opening_slots_in_use(&self, config: &OpeningConfig) -> Vec<(SlotId, u32)> {
        let slots: Vec<SlotId> = match config.source {
            RepSourceKind::File => Vec::new(),
            RepSourceKind::CurrentBase => vec![self.bases.current()],
            RepSourceKind::TypedBase => {
                let (white, black) = pick_typed(&self.opening_typed_slots(), config.side);
                white.into_iter().chain(black).collect()
            }
        };
        slots
            .into_iter()
            .filter_map(|id| Some((id, self.bases.slot(id)?.total())))
            .collect()
    }

    fn opening_start(
        &mut self,
        config: OpeningConfig,
        seed: u64,
        today: u32,
        effects: &mut Vec<Effect>,
    ) {
        // Never over a session that is on: it would lose the parked game.
        if self.flow.opening.machine.is_live() {
            return;
        }
        self.opening_load_store(effects);
        if self.opening_start_refused() || self.modal_open() {
            self.flow.opening.machine.set_notice(OpeningNotice::Busy);
            return;
        }
        // The game of the editor is parked, not saved: the user is asked
        // first whether to save it (D2229).
        if !self.opening_editor_is_ours() && self.needs_confirmation() {
            let request = OpeningStartRequest {
                config,
                seed,
                today,
            };
            self.proceed_or_ask(Pending::OpeningStart(Box::new(request)), effects);
            return;
        }
        self.opening_begin(config, seed, today, effects);
    }

    /// The start after the question about the game was answered (D2229).
    pub(crate) fn opening_start_confirmed(
        &mut self,
        config: OpeningConfig,
        seed: u64,
        today: u32,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.opening.machine.is_live() {
            return;
        }
        if self.opening_start_refused() {
            self.flow.opening.machine.set_notice(OpeningNotice::Busy);
            return;
        }
        self.opening_begin(config, seed, today, effects);
    }

    fn opening_begin(
        &mut self,
        config: OpeningConfig,
        seed: u64,
        today: u32,
        effects: &mut Vec<Effect>,
    ) {
        let source = self.opening_source(&config);
        let in_use = self.opening_slots_in_use(&config);
        let outs = self.flow.opening.machine.start(config, seed, today, source);
        if self.flow.opening.machine.is_live() {
            self.flow.opening.bases = in_use;
            self.opening_stop_engines(effects);
        }
        self.opening_run(outs, effects);
    }

    /// The engines would add their lines to the game of the line (D2232).
    fn opening_stop_engines(&mut self, effects: &mut Vec<Effect>) {
        let runs: Vec<_> = self.engine_runs().iter().map(|r| r.id).collect();
        for run in runs {
            self.stop_run(run, effects);
        }
    }

    /// Ends the session in the middle of another message, putting the game
    /// of the user back at once; the effects leave with that message
    /// (D2233).
    pub(crate) fn opening_halt(&mut self) {
        if !self.flow.opening.machine.is_live() {
            return;
        }
        let mut effects = Vec::new();
        self.opening_stop(&mut effects);
        self.flow.opening.deferred.extend(effects);
    }

    /// Ends the session and puts the game of the user back.
    pub(crate) fn opening_stop(&mut self, effects: &mut Vec<Effect>) {
        let outs = self.flow.opening.machine.close();
        self.opening_run(outs, effects);
    }

    fn opening_run(&mut self, outs: Vec<Out>, effects: &mut Vec<Effect>) {
        let mut save = false;
        for out in outs {
            match out {
                Out::Effect(effect) => effects.push(Effect::OpeningTrainer(effect)),
                Out::ShowGame { flipped } => self.opening_show(flipped),
                Out::Play(mv) => self.opening_play(mv),
                Out::SaveStore => save = true,
                Out::Closed => self.opening_restore(),
            }
        }
        if save {
            self.opening_save(effects);
        }
    }

    /// Writes the store, one write at a time so that an old one never lands
    /// after a new one.
    fn opening_save(&mut self, effects: &mut Vec<Effect>) {
        // A file of a newer version, or one that could not be read, is not
        // written over (D2225).
        if self.flow.opening.machine.store_read_only() {
            return;
        }
        let Some(prefs) = self.flow.prefs_path.as_deref() else {
            return;
        };
        // The file was not read yet: what is in memory is not the whole
        // score, and the file may be of a newer version (D2255).
        if !self.flow.opening.machine.store_ready() {
            self.flow.opening.save_waits_for_load = true;
            return;
        }
        if self.flow.opening.save_in_flight {
            self.flow.opening.resave = true;
            return;
        }
        let path = store_path(prefs);
        self.flow.opening.save_in_flight = true;
        effects.push(Effect::OpeningTrainer(OpeningEffect::SaveStore {
            path,
            store: Box::new(self.flow.opening.machine.store().clone()),
        }));
    }

    /// Puts a new game in the editor, parking the game of the user the
    /// first time (D2229), and turns the board to the side of the user.
    fn opening_show(&mut self, flipped: bool) {
        if !self.opening_editor_is_ours() {
            self.flow.opening.parked = Some(Parked::capture(self));
            self.flow.opening.parked_slot = Some(self.bases.current());
        }
        let mut game = Game::new();
        "Opening Trainer".clone_into(&mut game.tags_mut().event);
        self.session.replace_game(game);
        self.flow.opening.generation = Some(self.session.generation());
        self.origin = GameOrigin::New;
        self.collection = None;
        self.board_flipped = flipped;
        self.setup = None;
        self.info_dialog = None;
        self.chooser = None;
        self.discard_prompt = None;
    }

    fn opening_play(&mut self, mv: Move) {
        if !self.opening_editor_is_ours() {
            return;
        }
        drop(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
        drop(self.session.update(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
        self.session.mark_saved();
    }

    /// Keeps `parked` waiting in a slot (D2256): the one it came from first.
    /// Returns whether a slot took it.
    fn opening_stash(&mut self, parked: &Parked, from: SlotId) -> bool {
        let mut ids: Vec<SlotId> = vec![from];
        ids.extend(self.bases.iter().map(|(id, _)| id).filter(|id| *id != from));
        for id in ids {
            if let Some(slot) = self.bases.slot_mut(id)
                && slot.parked.is_none()
            {
                slot.parked = Some(crate::bases::Parked {
                    game: parked.game.clone(),
                    origin: parked.origin.clone(),
                    cursor: parked.cursor,
                    dirty: true,
                });
                return true;
            }
        }
        false
    }

    /// Puts the parked game back, if the editor still holds the line; a
    /// game with unsaved changes is never dropped in silence (D2229).
    fn opening_restore(&mut self) {
        let parked = self.flow.opening.parked.take();
        let from = self
            .flow
            .opening
            .parked_slot
            .take()
            .unwrap_or_else(|| self.bases.current());
        let ours = self.opening_editor_is_ours();
        self.flow.opening.generation = None;
        self.flow.opening.bases.clear();
        let Some(parked) = parked else {
            return;
        };
        if !ours {
            if !parked.at_stake() {
                return;
            }
            if self.needs_confirmation() {
                // The editor has work of its own: the parked game waits in
                // the slot it came from, else in another free one. When
                // there is no place the game of the editor stays (it is the
                // one that was just loaded) and the user is told (D2256).
                if !self.opening_stash(&parked, from) {
                    self.flow.opening.machine.open_dialog();
                    self.flow
                        .opening
                        .machine
                        .set_notice(OpeningNotice::GameNotRestored);
                }
                return;
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

    /// The moves of the user while it is his turn (D2230). Returns whether
    /// the message was consumed.
    pub(crate) fn opening_intercept(
        &mut self,
        message: &GameMessage,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if !self.flow.opening.machine.is_live() || !self.opening_editor_is_ours() {
            return false;
        }
        // The trial backup would be destroyed by the next game the trainer
        // puts in the editor, in any phase (D2378).
        if matches!(message, GameMessage::EnterTrial { .. }) {
            self.flow.opening.machine.set_notice(OpeningNotice::Locked);
            return true;
        }
        if !self.flow.opening.machine.locks_editor() {
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
                self.flow.opening.machine.set_notice(OpeningNotice::Locked);
                return true;
            }
        }
        let (session_effects, played) = self.session.update_capturing(message.clone());
        effects.extend(session_effects);
        if let Some(mv) = played {
            let outs = self.flow.opening.machine.user_move(mv);
            self.opening_run(outs, effects);
        }
        true
    }

    /// After every message: the session ends when the editor was taken over
    /// or another mode started (D2229, D2232, D2237).
    pub(crate) fn opening_guard(&mut self, effects: &mut Vec<Effect>) {
        effects.append(&mut self.flow.opening.deferred);
        if !self.flow.opening.machine.is_live() {
            return;
        }
        // While the repertoire is being read, a database that was closed or
        // changed ends the session; afterwards the repertoire is a copy and
        // does not depend on it (D2237).
        if self.flow.opening.machine.phase() == OpeningPhase::Building {
            let changed = self.flow.opening.bases.iter().any(|(id, total)| {
                self.bases
                    .slot(*id)
                    .is_none_or(|slot| slot.total() != *total)
            });
            if changed {
                let outs = self
                    .flow
                    .opening
                    .machine
                    .abort(OpeningNotice::SourceChanged);
                self.opening_run(outs, effects);
                return;
            }
        } else {
            self.flow.opening.bases.clear();
        }
        let taken = self.flow.opening.generation.is_some() && !self.opening_editor_is_ours();
        if taken || self.opening_blocked() {
            self.opening_stop(effects);
            return;
        }
        self.opening_stop_engines(effects);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_takes_the_white_base_and_falls_back_to_the_either_color_base() {
        let slots = [(1, 5), (2, TYPE_OPENINGS_EITHER), (3, TYPE_OPENINGS_WHITE)];
        assert_eq!(pick_typed(&slots, TrainSide::White), (Some(3), None));
        let only_either = [(1, 5), (2, TYPE_OPENINGS_EITHER)];
        assert_eq!(pick_typed(&only_either, TrainSide::White), (Some(2), None));
        assert_eq!(pick_typed(&only_either, TrainSide::Black), (None, Some(2)));
    }

    #[test]
    fn both_sides_take_one_base_each_and_the_first_of_a_type_wins() {
        let slots = [
            (1, TYPE_OPENINGS_BLACK),
            (2, TYPE_OPENINGS_WHITE),
            (3, TYPE_OPENINGS_WHITE),
        ];
        assert_eq!(pick_typed(&slots, TrainSide::Both), (Some(2), Some(1)));
    }

    #[test]
    fn no_typed_base_gives_nothing() {
        let slots = [(1, 4), (2, 5)];
        assert_eq!(pick_typed(&slots, TrainSide::Both), (None, None));
        assert_eq!(pick_typed::<u8>(&[], TrainSide::White), (None, None));
    }

    #[test]
    fn the_key_of_a_repertoire_has_the_colour_and_is_empty_without_a_path() {
        assert_eq!(rep_key("/a.si4", Color::White), "/a.si4|w");
        assert_eq!(rep_key("/a.si4", Color::Black), "/a.si4|b");
        assert_eq!(rep_key("", Color::White), "");
    }

    fn game_with_move() -> Game {
        let mut game = Game::new();
        let mv = mychess_core::Position::new()
            .parse_uci("e2e4")
            .expect("uci");
        let root = game.root();
        game.add_move(root, mv).expect("legal");
        game
    }

    fn parked_from() -> Parked {
        let game = game_with_move();
        Parked {
            cursor: game.root(),
            game,
            origin: GameOrigin::New,
            flipped: false,
            dirty: true,
            collection: None,
        }
    }

    /// A state whose editor has unsaved work of its own and whose trainer
    /// believes the editor was taken by something else.
    fn state_with_a_taken_editor() -> AppState {
        let mut state = AppState::default();
        let mv = state.session.position().parse_uci("d2d4").expect("uci");
        drop(state.session.update(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
        assert!(state.needs_confirmation());
        state.flow.opening.generation = Some(u64::MAX);
        state
    }

    fn occupied() -> crate::bases::Parked {
        let game = game_with_move();
        crate::bases::Parked {
            cursor: game.root(),
            game,
            origin: GameOrigin::New,
            dirty: true,
        }
    }

    #[test]
    fn a_parked_game_waits_in_the_slot_it_came_from_when_the_editor_has_work() {
        let mut state = state_with_a_taken_editor();
        let origin = state.bases.current();
        state.flow.opening.parked = Some(parked_from());
        state.flow.opening.parked_slot = Some(origin);
        state.opening_restore();
        assert!(state.session.is_dirty(), "the editor was left alone");
        let slot = state.bases.slot(origin).expect("slot");
        assert!(slot.parked.as_ref().is_some_and(|p| p.dirty));
        assert!(!state.opening_dialog_open(), "nothing to tell");
    }

    #[test]
    fn a_parked_game_that_finds_no_room_never_replaces_the_game_of_the_editor() {
        let mut state = state_with_a_taken_editor();
        let origin = state.bases.current();
        let ids: Vec<SlotId> = state.bases.iter().map(|(id, _)| id).collect();
        for id in ids {
            state.bases.slot_mut(id).expect("slot").parked = Some(occupied());
        }
        state.flow.opening.parked = Some(parked_from());
        state.flow.opening.parked_slot = Some(origin);
        state.opening_restore();
        assert_eq!(
            state.session.game().mainline().count(),
            1,
            "the d4 of the editor is still there"
        );
        assert!(state.session.is_dirty());
        assert!(state.opening_dialog_open());
        assert_eq!(
            state.opening_view().notice,
            Some(OpeningNotice::GameNotRestored)
        );
    }

    #[test]
    fn a_parked_game_without_changes_is_let_go_when_the_editor_has_work() {
        let mut state = state_with_a_taken_editor();
        let origin = state.bases.current();
        let mut parked = parked_from();
        parked.dirty = false;
        state.flow.opening.parked = Some(parked);
        state.flow.opening.parked_slot = Some(origin);
        state.opening_restore();
        assert!(state.session.is_dirty());
        assert!(state.bases.slot(origin).expect("slot").parked.is_none());
        assert!(!state.opening_dialog_open());
    }

    #[test]
    fn an_edit_from_outside_does_not_change_the_line_while_the_user_is_to_move() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("rep.pgn");
        std::fs::write(&path, "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 *\n").expect("write");
        let mut state = AppState::default();
        let mut effects = Vec::new();
        let config = OpeningConfig {
            source: RepSourceKind::File,
            file: Some(path),
            ..OpeningConfig::default()
        };
        state.opening_message(
            OpeningMessage::Start {
                config: Box::new(config),
                seed: 1,
                today: 1,
            },
            &mut effects,
        );
        let build_job = effects.iter().find_map(|e| match e {
            Effect::OpeningTrainer(OpeningEffect::Build { request, job }) => {
                Some((*request, job.clone()))
            }
            _ => None,
        });
        let (request, job) = build_job.expect("a build");
        let result = super::super::build::build(&job).map(Box::new);
        state.opening_message(OpeningMessage::Built { request, result }, &mut effects);
        assert_eq!(state.opening_phase(), OpeningPhase::UserTurn);
        assert!(state.opening_editor_locked());

        let before = state.session.game().mainline().count();
        let mv = state.session.position().parse_uci("d2d4").expect("uci");
        state.apply_game(
            GameMessage::PlayMove {
                mv,
                options: PlayOptions::default(),
            },
            &mut effects,
        );
        assert_eq!(state.session.game().mainline().count(), before);
    }

    #[test]
    fn a_trial_is_refused_while_the_session_is_on_outside_the_phase_that_locks_the_editor() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("rep.pgn");
        std::fs::write(&path, "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 *\n").expect("write");
        let mut state = AppState::default();
        let mut effects = Vec::new();
        let config = OpeningConfig {
            source: RepSourceKind::File,
            file: Some(path),
            ..OpeningConfig::default()
        };
        state.opening_message(
            OpeningMessage::Start {
                config: Box::new(config),
                seed: 1,
                today: 1,
            },
            &mut effects,
        );
        let (request, job) = effects
            .iter()
            .find_map(|e| match e {
                Effect::OpeningTrainer(OpeningEffect::Build { request, job }) => {
                    Some((*request, job.clone()))
                }
                _ => None,
            })
            .expect("a build");
        // While the repertoire is built the editor is not locked.
        assert!(state.opening_live());
        assert!(!state.flow.opening.machine.locks_editor());
        state.flow.opening.generation = Some(state.session.generation());
        assert!(
            state.opening_intercept(
                &GameMessage::EnterTrial { null_move: false },
                &mut Vec::new(),
            ),
            "the trial is taken by the trainer"
        );
        assert!(!state.session.in_trial());
        drop((request, job));
    }

    #[test]
    fn a_start_in_trial_mode_is_refused_and_the_backup_is_kept() {
        let mut state = AppState::default();
        drop(
            state
                .session
                .update(GameMessage::EnterTrial { null_move: false }),
        );
        let mut effects = Vec::new();
        state.opening_message(
            OpeningMessage::Start {
                config: Box::default(),
                seed: 1,
                today: 1,
            },
            &mut effects,
        );
        assert!(!state.opening_live());
        assert_eq!(state.opening_view().notice, Some(OpeningNotice::Busy));
        assert!(state.session.in_trial(), "the trial was not destroyed");
    }

    #[test]
    fn a_start_under_a_dialog_is_refused() {
        let mut state = AppState::default();
        state.discard_prompt = Some(crate::flow::DiscardPrompt {
            pending: Pending::Quit,
        });
        let mut effects = Vec::new();
        state.opening_message(
            OpeningMessage::Start {
                config: Box::default(),
                seed: 1,
                today: 1,
            },
            &mut effects,
        );
        assert!(!state.opening_live());
        assert_eq!(state.opening_view().notice, Some(OpeningNotice::Busy));
        assert!(state.discard_prompt.is_some());
    }

    #[test]
    fn the_exit_waits_for_a_write_that_waits_for_the_file() {
        let mut state = AppState::default();
        state.flow.opening.save_waits_for_load = true;
        let mut effects = Vec::new();
        assert!(state.opening_quit_wait(&mut effects));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::QuitDeadline { .. }))
        );
        assert!(state.opening_quit_deadline());
        assert!(!state.flow.opening.save_waits_for_load);
        assert!(!state.opening_quit_wait(&mut Vec::new()));
    }
}
