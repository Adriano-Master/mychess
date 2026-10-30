//! The game against an engine in the state (spec M8-1, phase A).
//!
//! The rules live in [`crate::play`] (a pure controller); this module is the
//! glue: it picks and prepares the engine, turns the controller's requests
//! into engine commands, hands the engine's events back, mirrors the game
//! into the editor session, and puts the engine back as it was. Nothing here
//! blocks: every wait is an engine event or a tick of the front-end, and
//! every command to the engine is an [`Effect`].
//!
//! Time comes from the messages (`at`) or, for the engine's events, from
//! [`AppState::play_now`], the single place that reads the clock (tests fix
//! it).

use std::time::{Duration, Instant};

use mychess_core::{Color, Role, Square};
use mychess_engine::{
    Bound, EngineCommand, EngineEvent, EngineWarning, OptionKind, OptionValue, Protocol, Score,
    SearchId,
};

mod coach;
mod ponder;
mod tactical;

use super::AppState;
use crate::effect::Effect;
use crate::engines::{CommandKind, EngineId, EngineRunState, RunId};
use crate::files::GameOrigin;
use crate::flow::AppNotice;
use crate::online::{Premove, PremoveSlot};
use crate::play::{
    BookFailure, ColorChoice, OpponentInfo, PlayBook, PlayConfig, PlayController, PlayMessage,
    PlayNotice, PlayOutput, PlayPhase, PlayRemark, PlaySetup, PlayView, StartFrom, Think,
    ThinkLimits,
};
use crate::session::{GameMessage, Nav};

/// The high bits of the labels of the searches of the game against an
/// engine (the annotation uses `0x4E4E...`, the modes `0x5452...`).
const TAG_BASE: u64 = 0x504C_0000_0000_0000;

/// How long past its time a search may stay silent before the engine is
/// given up (D1239).
pub(crate) const STALL_GRACE: Duration = Duration::from_secs(30);

/// How long a search without a time limit (depth, nodes) may stay silent.
const SILENT_LIMIT: Duration = Duration::from_secs(120);

/// The longest a single move may take whatever the engine reports: an
/// engine that keeps sending `info` without a `bestmove` is given up at
/// this ceiling (depth and nodes modes) or at three times the time of the
/// move (movetime mode).
pub(crate) const MOVE_CEILING: Duration = Duration::from_secs(300);

/// The oldest cancelled searches kept.
const STALE_LIMIT: usize = 16;

/// Whether `tag` labels a search of the game against an engine.
pub(crate) fn is_play_tag(tag: u64) -> bool {
    tag & 0xFFFF_0000_0000_0000 == TAG_BASE
}

/// The bookkeeping of the game against an engine.
#[derive(Debug, Default)]
pub(crate) struct PlayFlow {
    active: Option<Active>,
    request_seq: u64,
    tag_seq: u64,
    /// Counter of the tags of the coach's searches; it never goes back, so
    /// a late report of an old game cannot match a new search (D2434).
    coach_tag_seq: u64,
    /// Searches that were stopped: what they report is dropped.
    stale: Vec<(RunId, SearchId)>,
    /// Tags of searches stopped before the engine labelled them.
    stale_tags: Vec<u64>,
    /// Tests fix the clock and the draws here.
    pub(crate) fixed_now: Option<Instant>,
    pub(crate) fixed_roll: Option<u64>,
    /// A position is being drawn for a tactical game (M8-7, D2460).
    picking: Option<tactical::Picking>,
    /// The premove kept while the engine thinks (spec M8-1, D4452).
    premove: PremoveSlot,
    /// The user switched the premove off for this session (`Default` is
    /// on, D4452).
    premove_off: bool,
}

#[derive(Debug)]
enum Stage {
    /// Waiting for the engine, and for the book if one was asked for.
    Starting { engine_ready: bool, book: BookState },
    /// The controller runs the game.
    Playing(Box<PlayController>),
}

#[derive(Debug)]
enum BookState {
    NotAsked,
    Loading(u64),
    Ready(Option<PlayBook>),
}

/// What to put back in the engine when the game is over.
#[derive(Debug, Default)]
struct Restore {
    analyzing: bool,
    multipv: Option<u8>,
    /// The strength options the game changed, with the value they had.
    strength: Vec<(String, OptionValue)>,
    /// The analyses of other executions that were stopped for the game
    /// (D1241).
    paused: Vec<RunId>,
    /// `SetPonder(true)` was sent: send `SetPonder(false)` back (D2402).
    ponder: bool,
}

/// The search the opponent is working on.
#[derive(Debug)]
struct Thinking {
    tag: u64,
    search: Option<SearchId>,
    started: Instant,
    last_event: Instant,
    /// Silence allowed before the engine is given up.
    allowed: Duration,
    /// The longest the whole move may take, however chatty the engine is.
    ceiling: Duration,
    /// The last exact evaluation of the best line (White's point of view),
    /// handed to the controller with the move (D1504).
    score: Option<Score>,
}

#[derive(Debug)]
struct Active {
    stage: Stage,
    run: RunId,
    config: PlayConfig,
    restore: Restore,
    thinking: Option<Thinking>,
    /// A new engine was asked for after the old one was lost (D1239).
    switching: bool,
    /// The position a restart starts from, when it must not be read from
    /// the editor.
    start: Option<mychess_core::Position>,
    /// The opponent thinks on the player's time (M8-1d, D2403).
    pondering: Option<ponder::Pondering>,
    /// The auxiliary engine that judges the moves of the player (M8-1d).
    coach: Option<coach::CoachSlot>,
    /// What a tactical game keeps beside its configuration (M8-7).
    tactical: tactical::TacticalRun,
}

/// The value `name` had before the game touched it: the one saved for the
/// engine or, failing that, the default it declared. `None` for an option
/// that is not a check or a spin.
fn previous_option(
    options: &[mychess_engine::EngineOption],
    saved: &[(String, String)],
    name: &str,
) -> Option<OptionValue> {
    let option = options.iter().find(|o| o.name == name)?;
    let saved = saved
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, text)| crate::engines::option_value(text));
    match (&option.kind, saved) {
        (OptionKind::Check { .. }, Some(v @ OptionValue::Bool(_)))
        | (OptionKind::Spin { .. }, Some(v @ OptionValue::Int(_))) => Some(v),
        (OptionKind::Check { default }, _) => Some(OptionValue::Bool(*default)),
        (OptionKind::Spin { default, .. }, _) => Some(OptionValue::Int(*default)),
        _ => None,
    }
}

impl AppState {
    // ---- reading ----

    /// The current time for the events that carry none.
    fn play_now(&self) -> Instant {
        self.flow.play.fixed_now.unwrap_or_else(Instant::now)
    }

    /// Whether a game against an engine is on (starting, running, paused or
    /// waiting for an answer). While it is, the game cannot be edited.
    pub fn play_live(&self) -> bool {
        // A position being drawn for a tactical game holds the board too
        // (M8-7, D2460).
        if self.flow.play.picking.is_some() {
            return true;
        }
        match self.flow.play.active.as_ref().map(|a| &a.stage) {
            Some(Stage::Starting { .. }) => true,
            Some(Stage::Playing(c)) => c.is_live(),
            None => false,
        }
    }

    /// Whether a new engine is being waited for (tests).
    #[cfg(test)]
    pub(crate) fn play_switching(&self) -> bool {
        self.flow.play.active.as_ref().is_some_and(|a| a.switching)
    }

    /// Whether the front-end should send [`PlayMessage::Tick`] (every
    /// 100 ms): the clocks run, or the coach has a search in flight whose
    /// watchdog must keep running through a pause (D2435).
    pub fn play_wants_ticks(&self) -> bool {
        // The *watchdog* of the drawing of a position (M8-7, D2460).
        if self.flow.play.picking.is_some() {
            return true;
        }
        let Some(active) = self.flow.play.active.as_ref() else {
            return false;
        };
        match &active.stage {
            Stage::Playing(c) => {
                matches!(c.phase(), PlayPhase::Player | PlayPhase::Opponent)
                    || (c.is_live()
                        && active
                            .coach
                            .as_ref()
                            .is_some_and(coach::CoachSlot::is_searching))
            }
            Stage::Starting { .. } => false,
        }
    }

    /// The phase of the game, without reading the clock or building a
    /// [`PlayView`]: cheap enough for `view` and for every tick. `None`
    /// without a game (also while it is starting).
    pub fn play_phase(&self) -> Option<PlayPhase> {
        match self.flow.play.active.as_ref().map(|a| &a.stage) {
            Some(Stage::Playing(c)) => Some(c.phase()),
            _ => None,
        }
    }

    /// The configuration of the game against the engine (also once it has
    /// ended, until a new game replaces it); `None` without a game.
    pub fn play_config(&self) -> Option<&PlayConfig> {
        self.flow.play.active.as_ref().map(|a| &a.config)
    }

    /// The clocks of the game at `now` as `(white, black, countdown)`:
    /// what the strips need on each tick, without the rest of the
    /// [`PlayView`].
    pub fn play_clocks(&self, now: Instant) -> Option<(Duration, Duration, bool)> {
        let Some(Stage::Playing(c)) = self.flow.play.active.as_ref().map(|a| &a.stage) else {
            return None;
        };
        Some((
            c.shown(Color::White, now),
            c.shown(Color::Black, now),
            c.clock().is_countdown(),
        ))
    }

    /// Whether replacing the game in the editor (to start or restart a
    /// game against an engine) would lose unsaved changes: the same test
    /// the application makes for `UnsavedGame`.
    pub fn play_start_needs_discard(&self) -> bool {
        // A game with unsaved changes that the tactics training set aside
        // counts: the training ends first and gives it back (M8-7, D2466).
        self.needs_confirmation() || self.tactics_parked_at_stake()
    }

    /// What to draw: the clocks, the buttons and the question, at `now`.
    /// `None` without a game (also while it is starting).
    pub fn play_view(&self, now: Instant) -> Option<PlayView> {
        let Some(Stage::Playing(c)) = self.flow.play.active.as_ref().map(|a| &a.stage) else {
            return None;
        };
        let phase = c.phase();
        Some(PlayView {
            phase,
            player_color: c.player_color(),
            opponent: c.opponent().clone(),
            white_time: c.shown(Color::White, now),
            black_time: c.shown(Color::Black, now),
            countdown: c.clock().is_countdown(),
            question: match phase {
                PlayPhase::Asking(q) => Some(q),
                _ => None,
            },
            result: c.result(),
            can_resume: matches!(phase, PlayPhase::Paused { lost: false }),
            engine_lost: matches!(phase, PlayPhase::Paused { lost: true }),
            can_take_back: c.can_take_back()
                && self
                    .flow
                    .play
                    .active
                    .as_ref()
                    .is_none_or(|a| a.coach.is_none()),
            can_offer_draw: c.can_offer_draw(),
            pondering: phase == PlayPhase::Player
                && !self.assist_locked()
                && self
                    .flow
                    .play
                    .active
                    .as_ref()
                    .is_some_and(|a| a.pondering.is_some()),
            // Help of the engines family: hidden while the assistance is
            // locked (D2432).
            coach: self
                .flow
                .play
                .active
                .as_ref()
                .and_then(|a| a.coach.as_ref())
                .filter(|_| !self.assist_locked())
                .map(coach::CoachSlot::view),
        })
    }

    /// Whether moving the piece on `from` to `to` needs the piece of a
    /// promotion to be chosen.
    pub fn play_needs_promotion(&self, from: Square, to: Square) -> bool {
        let Some(Stage::Playing(c)) = self.flow.play.active.as_ref().map(|a| &a.stage) else {
            return false;
        };
        c.position()
            .legal_moves_from(from)
            .iter()
            .any(|m| m.is_promotion() && m.to() == Some(to))
    }

    /// Whether the premove against the engine is on (D4452).
    pub fn play_premove_enabled(&self) -> bool {
        !self.flow.play.premove_off
    }

    /// The premove kept against the engine, if any.
    pub fn play_premove(&self) -> Option<Premove> {
        self.flow.play.premove.get()
    }

    /// The position and the colour of the player when a premove can be
    /// entered now: the premove is on and the engine is thinking (D4452).
    pub fn play_premove_context(&self) -> Option<(mychess_core::Position, Color)> {
        if self.flow.play.premove_off {
            return None;
        }
        let Some(Stage::Playing(c)) = self.flow.play.active.as_ref().map(|a| &a.stage) else {
            return None;
        };
        (c.phase() == PlayPhase::Opponent).then(|| (c.position().clone(), c.player_color()))
    }

    /// Whether `run` is the engine of the game (the analysis does not drive
    /// it meanwhile, D1241).
    pub(crate) fn play_uses(&self, run: RunId) -> bool {
        self.flow.play.active.as_ref().is_some_and(|a| {
            (a.run == run || a.coach.as_ref().is_some_and(|c| c.run() == run)) && self.play_live()
        })
    }

    /// Refuses what would fight the game for the board or the engine.
    pub(super) fn play_busy(&mut self) {
        self.play_notice(PlayNotice::Busy);
    }

    fn play_notice(&mut self, notice: PlayNotice) {
        self.notice = Some(AppNotice::Play(notice));
    }

    fn play_send(&self, run: RunId, command: EngineCommand, effects: &mut Vec<Effect>) {
        effects.push(Effect::EngineCommand {
            run,
            command: Box::new(command),
            hub: self.engine_hub.clone(),
        });
    }

    // ---- messages ----

    pub(crate) fn play_message(&mut self, message: PlayMessage, effects: &mut Vec<Effect>) {
        match message {
            PlayMessage::Start(config) => self.play_start(*config, None, false, effects),
            PlayMessage::BookLoaded { request, result } => {
                self.play_book_loaded(request, result, effects);
            }
            PlayMessage::Picked { request, result } => {
                self.tactical_picked(request, result, effects);
            }
            PlayMessage::PlayerMove {
                from,
                to,
                promotion,
                at,
            } => self.play_player_move(from, to, promotion, at, effects),
            PlayMessage::Answer { yes, at } => {
                self.play_control(effects, |c| c.answer(yes, at));
            }
            PlayMessage::Resign(at) => {
                self.play_control(effects, |c| c.resign(at));
            }
            PlayMessage::Quit(at) => self.play_quit(at, effects),
            PlayMessage::Restart(at) => self.play_restart(at, false, effects),
            PlayMessage::Rematch(at) => {
                // Only a game that is over has a rematch (M8-12, D3101).
                if matches!(self.play_phase(), Some(PlayPhase::Ended(_))) {
                    self.play_restart(at, true, effects);
                }
            }
            PlayMessage::Pause(at) => {
                self.play_control(effects, |c| c.pause(at));
            }
            PlayMessage::Resume(at) => {
                self.play_control(effects, |c| c.resume(at));
            }
            PlayMessage::SwitchEngine(id, _) => self.play_switch(id, effects),
            PlayMessage::TakeBack(at) => {
                // The coach and the take back do not go together (D2427).
                if self
                    .flow
                    .play
                    .active
                    .as_ref()
                    .is_some_and(|a| a.coach.is_some())
                {
                    return self.play_notice(PlayNotice::CoachNoTakeBack);
                }
                self.play_control(effects, |c| c.take_back(at));
            }
            PlayMessage::OfferDraw(at) => {
                self.play_control(effects, |c| c.offer_draw(at));
            }
            PlayMessage::Tick(at) => self.play_tick(at, effects),
            PlayMessage::SetPremove {
                from,
                to,
                promotion,
            } => self.play_set_premove(from, to, promotion),
            PlayMessage::CancelPremove => {
                self.flow.play.premove.cancel();
            }
            PlayMessage::EnablePremove(on) => {
                self.flow.play.premove_off = !on;
                if !on {
                    self.flow.play.premove.cancel();
                }
            }
        }
    }

    /// Keeps a premove while the engine thinks (D4452); anything the piece
    /// could never do, or a turn that is not the engine's, is ignored.
    fn play_set_premove(&mut self, from: Square, to: Square, promotion: Option<Role>) {
        let Some((position, color)) = self.play_premove_context() else {
            return;
        };
        if let Some(premove) = Premove::enter(&position, color, from, to, promotion) {
            self.flow.play.premove.set(premove);
        }
    }

    /// The engine moved: plays the premove if it is legal now, drops it
    /// otherwise, silently (D4452). A premove that would leave the book or
    /// the line is dropped too (the question is not asked behind the
    /// player's back).
    fn play_try_premove(&mut self, at: Instant, effects: &mut Vec<Effect>) {
        let premove = &mut self.flow.play.premove;
        if !premove.is_set() {
            return;
        }
        let Some(Stage::Playing(controller)) = self.flow.play.active.as_mut().map(|a| &mut a.stage)
        else {
            premove.cancel();
            return;
        };
        if controller.phase() != PlayPhase::Player {
            premove.cancel();
            return;
        }
        let Some(mv) = premove.take_if_legal(controller.position()) else {
            return;
        };
        let mut outputs = controller.player_move(mv, at);
        if matches!(controller.phase(), PlayPhase::Asking(_)) {
            outputs = controller.answer(false, at);
        }
        self.play_outputs(outputs, effects);
    }

    /// Runs `f` on the controller and carries out what it asks.
    fn play_control(
        &mut self,
        effects: &mut Vec<Effect>,
        f: impl FnOnce(&mut PlayController) -> Vec<PlayOutput>,
    ) {
        let Some(Stage::Playing(controller)) = self.flow.play.active.as_mut().map(|a| &mut a.stage)
        else {
            return;
        };
        let outputs = f(controller);
        self.play_outputs(outputs, effects);
    }

    fn play_player_move(
        &mut self,
        from: Square,
        to: Square,
        promotion: Option<Role>,
        at: Instant,
        effects: &mut Vec<Effect>,
    ) {
        let Some(Stage::Playing(controller)) = self.flow.play.active.as_mut().map(|a| &mut a.stage)
        else {
            return;
        };
        let outputs = match controller.position().find_move(from, to, promotion) {
            Some(mv) => controller.player_move(mv, at),
            None => vec![PlayOutput::Notice(PlayRemark::IllegalMove)],
        };
        self.play_outputs(outputs, effects);
    }

    // ---- start ----

    fn play_start(
        &mut self,
        config: PlayConfig,
        start: Option<mychess_core::Position>,
        restart: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.play_live() {
            return self.play_notice(PlayNotice::AlreadyRunning);
        }
        // A game against an engine is help of the engines: never during an
        // online game, whatever the way the start came in (M8-7, D2467).
        if self.assist_locked() {
            return self.tactical_locked_notice();
        }
        if self.annotation_running()
            || self.training_playing()
            || self.tournament_live()
            || self.epd_running()
            || self.modal_open()
            || self.session.in_trial()
            || self.flow.open_base.is_some()
        {
            return self.play_notice(PlayNotice::Busy);
        }
        let config = config.normalized();
        let Some(entry) = self.engines.get(config.engine).cloned() else {
            return self.play_notice(PlayNotice::NoEngine);
        };
        if entry.protocol != Protocol::Uci {
            return self.play_notice(PlayNotice::NotUci);
        }
        if !Self::tactical_position_ok(&config) {
            return self.play_notice(PlayNotice::TacticalBadPosition);
        }
        // The problem of the tactics training would be taken as the game of
        // the user (D1931, D1964): the training ends first, with the game of
        // the user back in the editor. It ends **before** the question about
        // unsaved changes, so that a game of the user that the training had
        // set aside is seen by it and never replaced (M8-7, D2466).
        self.tactics_halt();
        // The game in the editor is replaced: ask the user first (D1236).
        if !restart && self.needs_confirmation() {
            return self.play_notice(PlayNotice::UnsavedGame);
        }
        // The position still has to be drawn (M8-7, D2460): the start goes
        // on with the answer.
        if let StartFrom::Pick(source) = config.start.clone() {
            return self.tactical_start_pick(config, &source, effects);
        }
        let Some((run, ready)) = self.play_engine_run(&entry, effects) else {
            return self.play_notice(PlayNotice::NoEngine);
        };
        let book = match &config.book {
            Some(choice) => {
                self.flow.play.request_seq = self.flow.play.request_seq.wrapping_add(1);
                let request = self.flow.play.request_seq;
                effects.push(Effect::LoadPlayBook {
                    path: choice.path.clone(),
                    selection: choice.selection,
                    request,
                });
                BookState::Loading(request)
            }
            None => BookState::NotAsked,
        };
        self.flow.play.active = Some(Active {
            stage: Stage::Starting {
                engine_ready: ready,
                book,
            },
            run,
            config,
            restore: Restore::default(),
            thinking: None,
            switching: false,
            start,
            pondering: None,
            coach: None,
            tactical: tactical::TacticalRun::default(),
        });
        self.play_try_begin(effects);
    }

    /// The execution of `entry`: a ready one, or a new one that will say
    /// when it is (`false`).
    fn play_engine_run(
        &mut self,
        entry: &crate::engines::EngineEntry,
        effects: &mut Vec<Effect>,
    ) -> Option<(RunId, bool)> {
        let existing = self
            .engine_runs
            .of_engine(entry.id)
            .map(|r| (r.id, r.state.clone()));
        match existing {
            Some((run, EngineRunState::Running)) => Some((run, true)),
            Some((run, EngineRunState::Starting)) => Some((run, false)),
            Some((run, EngineRunState::Failed(_))) => {
                drop(self.engine_runs.remove(run));
                let defaults = self.prefs.analysis.run_defaults();
                Some((
                    self.begin_run(entry.clone(), false, defaults, effects),
                    false,
                ))
            }
            Some(_) => None,
            None => {
                let defaults = self.prefs.analysis.run_defaults();
                Some((
                    self.begin_run(entry.clone(), false, defaults, effects),
                    false,
                ))
            }
        }
    }

    /// The engine of a game in `Starting` is ready.
    pub(crate) fn play_engine_ready(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if self.coach_owns(run) {
            return self.coach_ready(effects);
        }
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        if active.run != run {
            return;
        }
        if active.switching {
            return self.play_switch_ready(effects);
        }
        if let Stage::Starting { engine_ready, .. } = &mut active.stage {
            *engine_ready = true;
            self.play_try_begin(effects);
        }
    }

    fn play_book_loaded(
        &mut self,
        request: u64,
        result: Result<PlayBook, BookFailure>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(Active {
            stage: Stage::Starting { book, .. },
            ..
        }) = self.flow.play.active.as_mut()
        else {
            return;
        };
        if !matches!(book, BookState::Loading(r) if *r == request) {
            return;
        }
        *book = match result {
            Ok(loaded) => BookState::Ready(Some(loaded)),
            Err(_) => BookState::Ready(None),
        };
        let failed = matches!(book, BookState::Ready(None));
        if failed {
            self.play_notice(PlayNotice::BookUnreadable);
        }
        self.play_try_begin(effects);
    }

    /// Begins the game once the engine and the book are there.
    fn play_try_begin(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        let Stage::Starting { engine_ready, book } = &mut active.stage else {
            return;
        };
        if !*engine_ready {
            return;
        }
        let book = match book {
            BookState::Loading(_) => return,
            BookState::NotAsked => None,
            BookState::Ready(book) => book.take(),
        };
        self.play_begin(book, effects);
    }

    fn play_abort(&mut self, notice: PlayNotice, effects: &mut Vec<Effect>) {
        self.play_restore_engine(effects);
        self.flow.play.active = None;
        self.play_notice(notice);
    }

    #[allow(clippy::too_many_lines)] // one sequence: checks, engine, setup, mirror
    fn play_begin(&mut self, book: Option<PlayBook>, effects: &mut Vec<Effect>) {
        self.flow.play.premove.cancel();
        let Some(active) = self.flow.play.active.as_ref() else {
            return;
        };
        let (run, mut config, fixed_start) =
            (active.run, active.config.clone(), active.start.clone());
        let Some(engine_run) = self.engine_runs.get(run) else {
            return self.play_abort(PlayNotice::NoEngine, effects);
        };
        if engine_run.state != EngineRunState::Running {
            return self.play_abort(PlayNotice::NoEngine, effects);
        }
        let opponent = self.play_opponent(run);
        let analyzing = engine_run.analyzing;
        let engine_960 = engine_run
            .identity
            .as_ref()
            .is_some_and(|i| i.capabilities.chess960);
        // The strength of a tactical game comes from what the engine
        // declares (M8-7, D2452).
        let declared: Vec<_> = engine_run
            .identity
            .as_ref()
            .map(|i| i.options.clone())
            .unwrap_or_default();
        let (depth_cap, tactical_notice) = Self::tactical_plan(&mut config, &declared);
        if let Some(active) = self.flow.play.active.as_mut() {
            active.config = config.clone();
            active.tactical.depth_cap = depth_cap;
            active.tactical.adjusted = None;
        }
        let mut restore = self.play_prepare_engine(run, &config, analyzing, effects);
        if let Some(notice) = tactical_notice {
            self.play_notice(notice);
        }
        restore.paused = self.play_pause_others(run, effects);
        let roll = self.play_roll();
        let start = match config.start {
            StartFrom::CurrentPosition => {
                fixed_start.unwrap_or_else(|| self.session.position().clone())
            }
            StartFrom::Position(ref position) => (**position).clone(),
            _ => mychess_core::Position::new(),
        };
        // UCI_Chess960 is switched on by the session; an engine without the
        // option cannot play the game (M9-6, D4505).
        if start.variant() == mychess_core::Variant::Chess960 && !engine_960 {
            return self.play_abort(PlayNotice::Chess960Unsupported, effects);
        }
        let setup = PlaySetup {
            player_color: config.color.resolve(roll),
            start,
            opponent,
            player_name: self.prefs.play.player_name.clone(),
            date: crate::effect::today_utc(),
            book,
            seed: roll,
            config,
        };
        let now = self.play_now();
        if let Ok((controller, outputs)) = PlayController::new(setup, now) {
            let flipped = controller.player_color() == Color::Black;
            if let Some(active) = self.flow.play.active.as_mut() {
                active.restore = restore;
                active.stage = Stage::Playing(Box::new(controller));
            }
            self.coach_start(effects);
            self.board_flipped = flipped;
            self.origin = GameOrigin::New;
            self.collection = None;
            self.note_current_game(None);
            self.play_mirror();
            self.play_outputs(outputs, effects);
        } else {
            if let Some(active) = self.flow.play.active.as_mut() {
                active.restore = restore;
            }
            self.play_abort(PlayNotice::BadOpening, effects);
        }
    }

    fn play_roll(&mut self) -> u64 {
        if let Some(roll) = self.flow.play.fixed_roll {
            return roll;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        // Only a draw for the colour and the book: the low bits are enough.
        u64::try_from(nanos & u128::from(u64::MAX)).unwrap_or(1)
    }

    fn play_opponent(&self, run: RunId) -> OpponentInfo {
        let r = self.engine_runs.get(run);
        let name = r
            .and_then(|r| r.identity.as_ref().map(|i| i.name.clone()))
            .or_else(|| {
                r.and_then(|r| self.engines.get(r.engine))
                    .map(|e| e.name.clone())
            })
            .unwrap_or_else(|| "?".to_owned());
        OpponentInfo { name, rating: None }
    }

    /// Sets the engine up for the game: fresh game, one line, the strength
    /// options it declares (D1237). Returns what to put back.
    fn play_prepare_engine(
        &mut self,
        run: RunId,
        config: &PlayConfig,
        analyzing: bool,
        effects: &mut Vec<Effect>,
    ) -> Restore {
        let mut restore = Restore {
            analyzing,
            ..Restore::default()
        };
        let entry = self
            .engine_runs
            .get(run)
            .and_then(|r| self.engines.get(r.engine))
            .cloned();
        let options = self
            .engine_runs
            .get(run)
            .and_then(|r| r.identity.as_ref())
            .map(|i| i.options.clone())
            .unwrap_or_default();
        if let Some(r) = self.engine_runs.get_mut(run) {
            r.analyzing = false;
            r.sent = None;
            r.seen_game = None;
        }
        self.play_send(run, EngineCommand::NewGame, effects);
        if config.ponder {
            if options
                .iter()
                .any(|o| o.name == "Ponder" && matches!(o.kind, OptionKind::Check { .. }))
            {
                restore.ponder = true;
                self.play_send(run, EngineCommand::SetPonder(true), effects);
            } else {
                self.play_notice(PlayNotice::PonderUnsupported);
            }
        }
        if let Some(entry) = entry.as_ref()
            && entry.multipv != 1
        {
            restore.multipv = Some(entry.multipv);
            self.play_send(run, EngineCommand::SetMultiPv(1), effects);
        }
        let spin = |name: &str| {
            options
                .iter()
                .find_map(|o| match (&o.kind, o.name == name) {
                    (OptionKind::Spin { min, max, .. }, true) => Some((*min, *max)),
                    _ => None,
                })
        };
        let declares = |name: &str| options.iter().any(|o| o.name == name);
        let saved: Vec<(String, String)> = entry
            .as_ref()
            .map(|e| e.options.clone())
            .unwrap_or_default();
        // What the option was: the value saved for the engine or, failing
        // that, the default it declared.
        let before = |name: &str| previous_option(&options, &saved, name);
        if let Some(elo) = config.strength.elo
            && declares("UCI_LimitStrength")
            && let Some((min, max)) = spin("UCI_Elo")
        {
            for name in ["UCI_LimitStrength", "UCI_Elo"] {
                if let Some(value) = before(name) {
                    restore.strength.push((name.to_owned(), value));
                }
            }
            self.play_send(
                run,
                EngineCommand::SetOption {
                    name: "UCI_LimitStrength".to_owned(),
                    value: OptionValue::Bool(true),
                },
                effects,
            );
            self.play_send(
                run,
                EngineCommand::SetOption {
                    name: "UCI_Elo".to_owned(),
                    value: OptionValue::Int(elo.clamp(min, max)),
                },
                effects,
            );
        }
        if let Some(skill) = config.strength.skill
            && let Some((min, max)) = spin("Skill Level")
        {
            if let Some(value) = before("Skill Level") {
                restore.strength.push(("Skill Level".to_owned(), value));
            }
            self.play_send(
                run,
                EngineCommand::SetOption {
                    name: "Skill Level".to_owned(),
                    value: OptionValue::Int(skill.clamp(min, max)),
                },
                effects,
            );
        }
        restore
    }

    /// Puts everything back as it was before the game: the engine of the
    /// game and the analyses that were stopped for it.
    fn play_restore_engine(&mut self, effects: &mut Vec<Effect>) {
        let paused = self
            .flow
            .play
            .active
            .as_mut()
            .map(|a| std::mem::take(&mut a.restore.paused))
            .unwrap_or_default();
        self.play_restore_run(effects);
        self.coach_restore(effects);
        self.play_resume_others(paused, effects);
    }

    /// Puts the engine of the game back as it was before the game.
    fn play_restore_run(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        let (run, restore) = (active.run, std::mem::take(&mut active.restore));
        let thinking = active.thinking.take();
        let pondering = active.pondering.take();
        let alive = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if !alive {
            return;
        }
        if let Some(t) = thinking {
            if let Some(id) = t.search {
                self.play_remember(run, id);
            } else {
                self.play_remember_tag(t.tag);
            }
            self.play_send(run, EngineCommand::Stop, effects);
        } else if let Some(p) = pondering {
            self.play_forget_ponder(run, &p);
            self.play_send(run, EngineCommand::Stop, effects);
        }
        if restore.ponder {
            self.play_send(run, EngineCommand::SetPonder(false), effects);
        }
        if let Some(lines) = restore.multipv {
            self.play_send(run, EngineCommand::SetMultiPv(lines), effects);
        }
        for (name, value) in restore.strength {
            self.play_send(run, EngineCommand::SetOption { name, value }, effects);
        }
        if let Some(r) = self.engine_runs.get_mut(run) {
            r.analyzing = restore.analyzing;
            r.sent = None;
        }
    }

    /// Stops the analysis of every other execution for the length of the
    /// game (D1241). Returns the ones stopped.
    fn play_pause_others(&mut self, game_run: RunId, effects: &mut Vec<Effect>) -> Vec<RunId> {
        let runs: Vec<RunId> = self
            .engine_runs
            .as_slice()
            .iter()
            .filter(|r| r.id != game_run && r.state == EngineRunState::Running && r.analyzing)
            .map(|r| r.id)
            .collect();
        for run in &runs {
            self.analysis_toggle(*run, effects);
        }
        runs
    }

    /// Starts again the analyses that [`AppState::play_pause_others`]
    /// stopped, those that still exist and were not turned on meanwhile.
    fn play_resume_others(&mut self, paused: Vec<RunId>, effects: &mut Vec<Effect>) {
        for run in paused {
            let stopped = self
                .engine_runs
                .get(run)
                .is_some_and(|r| r.state == EngineRunState::Running && !r.analyzing);
            if stopped {
                self.analysis_toggle(run, effects);
            }
        }
    }

    /// Remembers the tag of a search stopped before the engine labelled it,
    /// keeping the newest [`STALE_LIMIT`].
    fn play_remember_tag(&mut self, tag: u64) {
        let tags = &mut self.flow.play.stale_tags;
        if tags.len() >= STALE_LIMIT {
            tags.remove(0);
        }
        tags.push(tag);
    }

    fn play_remember(&mut self, run: RunId, search: SearchId) {
        let stale = &mut self.flow.play.stale;
        if stale.len() >= STALE_LIMIT {
            stale.remove(0);
        }
        stale.push((run, search));
    }

    // ---- the controller's outputs ----

    fn play_outputs(&mut self, outputs: Vec<PlayOutput>, effects: &mut Vec<Effect>) {
        for output in outputs {
            let Some(run) = self.flow.play.active.as_ref().map(|a| a.run) else {
                return;
            };
            match output {
                PlayOutput::Think(think) => self.play_think(run, &think, effects),
                PlayOutput::StopThinking => self.play_stop_thinking(run, effects),
                PlayOutput::Moved(record) => {
                    self.play_mirror();
                    self.coach_moved(&record, effects);
                }
                PlayOutput::TookBack => {
                    self.flow.play.premove.cancel();
                    self.play_mirror();
                    self.play_ponder_drop(effects);
                }
                PlayOutput::Notice(remark) => self.play_remark(remark, effects),
                PlayOutput::ClearOpponentHash => self.play_clear_hash(run, effects),
                PlayOutput::Ended(end) => {
                    self.flow.play.premove.cancel();
                    self.play_mirror();
                    self.session.mark_altered();
                    self.tactical_ended(end, effects);
                    self.play_restore_engine(effects);
                    self.play_notice(PlayNotice::Ended(end));
                }
                // `Ask` needs nothing here (the view shows the question);
                // `PlayOutput` is non-exhaustive.
                _ => {}
            }
        }
        self.play_ponder_check(effects);
    }

    fn play_remark(&mut self, remark: PlayRemark, effects: &mut Vec<Effect>) {
        let notice = match remark {
            PlayRemark::IllegalMove => PlayNotice::IllegalMove,
            PlayRemark::NotYourTurn => PlayNotice::NotYourTurn,
            PlayRemark::MoveTakenBack => PlayNotice::MoveTakenBack,
            PlayRemark::NothingToTakeBack => PlayNotice::NothingToTakeBack,
            PlayRemark::DrawNotAllowed => PlayNotice::DrawNotAllowed,
            PlayRemark::DrawDeclined => PlayNotice::DrawDeclined,
            PlayRemark::OpponentLost => {
                // The engine is not answering: its search, if any, is moot.
                if let Some(run) = self.flow.play.active.as_ref().map(|a| a.run) {
                    self.play_stop_thinking(run, effects);
                }
                self.play_ponder_drop(effects);
                PlayNotice::EngineLost
            }
            // `PlayRemark` is non-exhaustive.
            #[allow(unreachable_patterns)]
            _ => return,
        };
        self.play_notice(notice);
    }

    /// A fresh tag for a search of the game (D1238).
    fn play_next_tag(&mut self) -> u64 {
        self.flow.play.tag_seq = self.flow.play.tag_seq.wrapping_add(1);
        TAG_BASE | (self.flow.play.tag_seq & 0x0000_FFFF_FFFF_FFFF)
    }

    /// The silence and the total time allowed to a search for `think`.
    fn think_allowance(think: &Think) -> (Duration, Duration) {
        match think.limits {
            ThinkLimits::MoveTime(t) => (
                t.saturating_add(STALL_GRACE),
                t.saturating_mul(3).saturating_add(STALL_GRACE),
            ),
            ThinkLimits::Clock(c) => {
                let mine = match think.side {
                    Color::White => c.white_time,
                    Color::Black => c.black_time,
                };
                let limit = mine.saturating_add(STALL_GRACE);
                (limit, limit)
            }
            _ => (SILENT_LIMIT, MOVE_CEILING),
        }
    }

    fn play_think(&mut self, run: RunId, think: &Think, effects: &mut Vec<Effect>) {
        // The player played what the opponent was pondering on: the
        // search goes on as the real one (D2404).
        if self.play_ponder_hit(run, think, effects) {
            return;
        }
        let tag = self.play_next_tag();
        let (allowed, ceiling) = Self::think_allowance(think);
        let now = self.play_now();
        if let Some(active) = self.flow.play.active.as_mut() {
            active.thinking = Some(Thinking {
                tag,
                search: None,
                started: now,
                last_event: now,
                allowed,
                ceiling,
                score: None,
            });
        }
        let mut request = think.to_request(tag);
        self.tactical_cap(&mut request);
        self.play_send(run, EngineCommand::Search(request), effects);
    }

    fn play_stop_thinking(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let Some(thinking) = self
            .flow
            .play
            .active
            .as_mut()
            .and_then(|a| a.thinking.take())
        else {
            return;
        };
        match thinking.search {
            Some(id) => self.play_remember(run, id),
            None => self.play_remember_tag(thinking.tag),
        }
        let alive = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if alive {
            self.play_send(run, EngineCommand::Stop, effects);
        }
    }

    fn play_clear_hash(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let has_button = self
            .engine_runs
            .get(run)
            .and_then(|r| r.identity.as_ref())
            .is_some_and(|i| {
                i.options
                    .iter()
                    .any(|o| o.name == "Clear Hash" && matches!(o.kind, OptionKind::Button))
            });
        if has_button {
            self.play_send(
                run,
                EngineCommand::SetOption {
                    name: "Clear Hash".to_owned(),
                    value: OptionValue::Bool(true),
                },
                effects,
            );
        }
    }

    /// Copies the game of the controller to the editor, cursor at the end.
    fn play_mirror(&mut self) {
        let Some(Stage::Playing(controller)) = self.flow.play.active.as_ref().map(|a| &a.stage)
        else {
            return;
        };
        let game = controller.game().clone();
        self.session.replace_game(game);
        drop(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
    }

    // ---- quit, switch, tick ----

    /// Starts again with the configuration of the game that was on. With
    /// `swap`, the player takes the other colour (a rematch, M8-12, D3101).
    fn play_restart(&mut self, at: Instant, swap: bool, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_ref() else {
            return;
        };
        // "From the current position" means the start of the game
        // that was on, not wherever it ended (D1227).
        let (start, played) = match &active.stage {
            Stage::Playing(c) => (Some(c.setup().start.clone()), Some(c.player_color())),
            Stage::Starting { .. } => (active.start.clone(), None),
        };
        let mut config = active.config.clone();
        self.tactical_restart_config(&mut config);
        if swap && let Some(color) = played {
            // The colour really played, also when the setting was a roll.
            config.color = match color {
                Color::White => ColorChoice::Black,
                Color::Black => ColorChoice::White,
            };
        }
        self.play_quit(at, effects);
        self.play_start(config, start, true, effects);
    }

    fn play_quit(&mut self, at: Instant, effects: &mut Vec<Effect>) {
        self.flow.play.premove.cancel();
        self.tactical_cancel_pick();
        self.play_control(effects, |c| c.quit(at));
        self.play_restore_engine(effects);
        self.flow.play.active = None;
    }

    fn play_switch(&mut self, id: EngineId, effects: &mut Vec<Effect>) {
        if !self.play_is_lost() {
            return;
        }
        let Some(entry) = self.engines.get(id).cloned() else {
            return self.play_notice(PlayNotice::NoEngine);
        };
        if entry.protocol != Protocol::Uci {
            return self.play_notice(PlayNotice::NotUci);
        }
        let Some((run, ready)) = self.play_engine_run(&entry, effects) else {
            return self.play_notice(PlayNotice::NoEngine);
        };
        // The old engine, if it is still there, goes back as it was (the
        // analyses stopped for the game stay stopped).
        let paused = self
            .flow
            .play
            .active
            .as_mut()
            .map(|a| std::mem::take(&mut a.restore.paused))
            .unwrap_or_default();
        self.play_restore_run(effects);
        if let Some(active) = self.flow.play.active.as_mut() {
            active.restore.paused = paused;
            active.run = run;
            active.config.engine = id;
            active.switching = true;
        }
        if ready {
            self.play_switch_ready(effects);
        }
    }

    /// Whether the game is paused because its engine was lost.
    fn play_is_lost(&self) -> bool {
        matches!(
            self.flow.play.active.as_ref().map(|a| &a.stage),
            Some(Stage::Playing(c)) if c.phase() == (PlayPhase::Paused { lost: true })
        )
    }

    fn play_switch_ready(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        active.switching = false;
        if !self.play_is_lost() {
            // The game moved on (resigned, ended) while the engine started:
            // nothing to set up, the engine stays as it is.
            return self.play_restore_engine(effects);
        }
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        let (run, mut config) = (active.run, active.config.clone());
        let paused = std::mem::take(&mut active.restore.paused);
        let analyzing = self.engine_runs.get(run).is_some_and(|r| r.analyzing);
        // A new engine declares other options: the plan of a tactical game
        // is made again (M8-7, D2468).
        let declared: Vec<_> = self
            .engine_runs
            .get(run)
            .and_then(|r| r.identity.as_ref())
            .map(|i| i.options.clone())
            .unwrap_or_default();
        let (depth_cap, tactical_notice) = Self::tactical_plan(&mut config, &declared);
        if let Some(active) = self.flow.play.active.as_mut() {
            active.config = config.clone();
            active.tactical.depth_cap = depth_cap;
        }
        let mut restore = self.play_prepare_engine(run, &config, analyzing, effects);
        if let Some(notice) = tactical_notice {
            self.play_notice(notice);
        }
        restore.paused = paused;
        let opponent = self.play_opponent(run);
        let now = self.play_now();
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        active.restore = restore;
        let Stage::Playing(controller) = &mut active.stage else {
            return;
        };
        let outputs = controller.replace_opponent(opponent, now);
        self.play_mirror();
        self.play_outputs(outputs, effects);
    }

    fn play_tick(&mut self, at: Instant, effects: &mut Vec<Effect>) {
        self.tactical_pick_tick(at);
        let Some(active) = self.flow.play.active.as_ref() else {
            return;
        };
        let silent = active.thinking.as_ref().is_some_and(|t| {
            at.saturating_duration_since(t.last_event) > t.allowed
                || at.saturating_duration_since(t.started) > t.ceiling
        });
        if silent {
            // The engine stopped answering (D1239).
            return self.play_control(effects, |c| c.opponent_lost(at));
        }
        self.play_control(effects, |c| c.tick(at));
        self.play_ponder_tick(at, effects);
        self.coach_tick(at, effects);
    }

    /// Gives up the drawing of a start position, if one is pending (the
    /// program is quitting, M8-7, D2460).
    pub(crate) fn play_cancel_pick(&mut self) {
        self.tactical_cancel_pick();
    }

    /// Every engine was shut down (the program is quitting): the game, if
    /// any, is over for good and without a result.
    pub(crate) fn play_shutdown(&mut self, _effects: &mut Vec<Effect>) {
        self.flow.play.active = None;
    }

    // ---- the engine's events ----

    /// Gives an event of an engine to the game. Returns whether it took it.
    pub(crate) fn play_event(
        &mut self,
        run: RunId,
        event: &EngineEvent,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if self.coach_owns(run) {
            return self.coach_event(run, event, effects);
        }
        if self.flow.play.active.as_ref().is_none_or(|a| a.run != run) {
            // A cancelled search of a game that is gone.
            return self.play_drop_stale(run, event);
        }
        if self.play_drop_stale(run, event) {
            return true;
        }
        let now = self.play_now();
        if let Some(taken) = self.ponder_event(event, effects) {
            return taken;
        }
        let Some(active) = self.flow.play.active.as_mut() else {
            return false;
        };
        match event {
            EngineEvent::SearchTagged { search, tag } if is_play_tag(*tag) => {
                match active.thinking.as_mut() {
                    Some(t) if t.tag == *tag => {
                        t.search = Some(*search);
                        t.last_event = now;
                    }
                    _ => {
                        self.flow.play.stale_tags.retain(|t| t != tag);
                        self.play_remember(run, *search);
                    }
                }
                true
            }
            // What is not about the search of the game (the analysis after
            // the game, say) goes on to the analysis.
            EngineEvent::SearchStarted(id) => match active.thinking.as_mut() {
                Some(t) => {
                    t.search.get_or_insert(*id);
                    t.last_event = now;
                    true
                }
                None => false,
            },
            EngineEvent::Info(info) => match active.thinking.as_mut() {
                Some(t) if t.search == Some(info.search) => {
                    t.last_event = now;
                    // The final evaluation is the last exact one of the
                    // best line (a bound says little about the move).
                    if info.multipv <= 1
                        && let Some((score, Bound::Exact)) = info.score
                    {
                        t.score = Some(score);
                    }
                    true
                }
                _ => false,
            },
            EngineEvent::BestMove(best) => {
                let ours = active
                    .thinking
                    .as_ref()
                    .is_some_and(|t| t.search == Some(best.search));
                if ours {
                    let score = active.thinking.take().and_then(|t| t.score);
                    let mv = best.mv;
                    self.play_control(effects, |c| c.opponent_moved(mv, None, now, score));
                    // The reply the engine expects: think on it while the
                    // player moves (D2403).
                    if mv.is_some() {
                        self.play_ponder_start(best.ponder, effects);
                    }
                    // The premove goes out now if it is still legal (D4452).
                    self.play_try_premove(now, effects);
                }
                ours
            }
            EngineEvent::Warning(EngineWarning::CommandRefused {
                command: "go" | "position",
                ..
            }) if active.thinking.as_ref().is_some_and(|t| t.search.is_none()) => {
                self.play_control(effects, |c| c.opponent_lost(now));
                false
            }
            _ => false,
        }
    }

    /// Drops what a stopped search reports. Returns whether `event` was
    /// such a report.
    fn play_drop_stale(&mut self, run: RunId, event: &EngineEvent) -> bool {
        if let EngineEvent::SearchTagged { search, tag } = event
            && is_play_tag(*tag)
        {
            let current = self.flow.play.active.as_ref().is_some_and(|a| {
                a.thinking.as_ref().is_some_and(|t| t.tag == *tag)
                    || a.pondering.as_ref().is_some_and(|p| p.tag == *tag)
            });
            if current {
                return false;
            }
            self.flow.play.stale_tags.retain(|t| t != tag);
            self.play_remember(run, *search);
            return true;
        }
        let id = match event {
            EngineEvent::SearchStarted(id) => *id,
            EngineEvent::Info(info) => info.search,
            EngineEvent::BestMove(best) => best.search,
            _ => return false,
        };
        let stale = &mut self.flow.play.stale;
        let Some(at) = stale.iter().position(|e| *e == (run, id)) else {
            return false;
        };
        if matches!(event, EngineEvent::BestMove(_)) {
            stale.remove(at);
        }
        true
    }

    /// The engine of the game ended, failed to start or was stopped.
    pub(crate) fn play_engine_gone(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if self.coach_owns(run) {
            return self.coach_gone(effects);
        }
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        if active.run != run {
            return;
        }
        active.thinking = None;
        active.pondering = None;
        active.switching = false;
        match &active.stage {
            Stage::Starting { .. } => {
                self.flow.play.active = None;
                self.play_notice(PlayNotice::NoEngine);
            }
            Stage::Playing(_) => {
                let now = self.play_now();
                self.play_control(effects, |c| c.opponent_lost(now));
            }
        }
    }

    /// A command to the engine of the game was refused or lost.
    pub(crate) fn play_command_failed(
        &mut self,
        run: RunId,
        kind: CommandKind,
        effects: &mut Vec<Effect>,
    ) {
        if kind != CommandKind::Search || !self.play_uses(run) {
            return;
        }
        if self.coach_owns(run) {
            return self.coach_command_failed(effects);
        }
        self.ponder_command_failed();
        let awaiting = self
            .flow
            .play
            .active
            .as_ref()
            .and_then(|a| a.thinking.as_ref())
            .is_some_and(|t| t.search.is_none());
        if awaiting {
            let now = self.play_now();
            self.play_control(effects, |c| c.opponent_lost(now));
        }
    }

    // ---- the game of the editor ----

    /// What happens to a message for the game while a game against the
    /// engine is on (D1240). Returns whether it must be dropped.
    ///
    /// Navigation pauses the game and goes through; anything that edits is
    /// refused, because the controller owns the game.
    pub(crate) fn play_refuses_game_message(
        &mut self,
        message: &GameMessage,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if !self.play_live() {
            return false;
        }
        let navigation = matches!(
            message,
            GameMessage::Navigate(_)
                | GameMessage::OpenVariationChooser
                | GameMessage::ChooserMove(_)
                | GameMessage::ChooserConfirm(_)
                | GameMessage::CloseVariationChooser
                | GameMessage::AutoplayTick { .. }
                | GameMessage::StopAutoplay
                | GameMessage::DismissPrompt
        );
        if !navigation {
            self.play_notice(PlayNotice::Locked);
            return true;
        }
        if !matches!(message, GameMessage::AutoplayTick { .. }) {
            let at = self.play_now();
            self.play_control(effects, |c| c.pause(at));
        }
        false
    }
}
