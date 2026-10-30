//! The threat, the shoot-out and the automove in the state (spec M4-8).
//!
//! The rules (what ends a game, the notices) live in [`crate::training`];
//! this module is the glue. Nothing here blocks: every wait is an engine
//! event or a timer [`Effect`], and every command to the engine is an
//! [`Effect`].
//!
//! One mode at a time, for the whole application (D495): the game is a
//! single one, and two engines playing it would only fight.

use std::time::Duration;

use mychess_core::{Move, NodeId, Position, ZobristHash};
use mychess_engine::{EngineCommand, EngineEvent, EngineWarning, SearchId, SearchRequest};

use super::AppState;
use crate::effect::Effect;
use crate::engines::{CommandKind, EngineRunState, RunId};
use crate::flow::AppNotice;
use crate::session::{GameMessage, PlayOptions};
use crate::training::{
    AUTOMOVE_RANGE, AUTOMOVE_RETRIES, AUTOMOVE_RETRY_DELAY, AutomoveState, DEFAULT_AUTOMOVE,
    EngineMode, ModeAvailability, ModeKind, ModeMessage, ModeNotice, SEARCH_STALL_GRACE,
    StopReason, TrainingHook, TrainingOutcome, run_hooks, terminal_reason,
};

/// The tags of the requests of the modes (the analysis counts from 1, the
/// annotation uses `0x4E4E...`).
const TAG_BASE: u64 = 0x5452_0000_0000_0000;

/// What the game looked like after the last change the mode made or saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    game: u64,
    cursor: NodeId,
    changes: u64,
}

/// The search the mode is waiting for.
#[derive(Debug)]
struct InFlight {
    tag: u64,
    search: Option<SearchId>,
    /// The engine said its `bestmove` was illegal.
    illegal: bool,
    /// The generation of the watchdog timer of this search (D855).
    watch: u64,
}

/// How many searches of older modes are remembered to drop what they report.
const STALE_LIMIT: usize = 16;

/// Whether `tag` is one the modes put on their searches.
fn is_mode_tag(tag: u64) -> bool {
    tag & 0xFFFF_0000_0000_0000 == TAG_BASE
}

#[derive(Debug)]
enum Kind {
    /// Trial mode on the null move at `node`.
    Threat { node: NodeId },
    ShootOut {
        /// The last answer was unplayable and the step was asked again.
        retried: bool,
    },
    Automove {
        state: AutomoveState,
        /// Unplayable answers in a row.
        attempts: u8,
        thinking_ms: u32,
    },
}

/// The mode in progress.
#[derive(Debug)]
pub(crate) struct Active {
    run: RunId,
    kind: Kind,
    generation: u64,
    /// Whether the engine was analysing the board before the mode.
    restore_analyzing: bool,
    seen: Seen,
    inflight: Option<InFlight>,
}

impl Active {
    fn plays(&self) -> bool {
        !matches!(self.kind, Kind::Threat { .. })
    }

    fn mode_kind(&self) -> ModeKind {
        match self.kind {
            Kind::Threat { .. } => ModeKind::Threat,
            Kind::ShootOut { .. } => ModeKind::ShootOut,
            Kind::Automove { .. } => ModeKind::Automove,
        }
    }
}

impl TrainingFlow {
    /// Remembers a search whose reports are dropped.
    fn note_stale(&mut self, run: RunId, search: SearchId) {
        if self.stale.len() >= STALE_LIMIT {
            self.stale.remove(0);
        }
        self.stale.push((run, search));
    }

    /// Gives up the search in flight: what it reports is dropped. Returns
    /// whether there was one.
    fn drop_inflight(&mut self) -> bool {
        let Some(flight) = self.active.as_mut().and_then(|a| a.inflight.take()) else {
            return false;
        };
        if let (Some(search), Some(run)) = (flight.search, self.active.as_ref().map(|a| a.run)) {
            self.note_stale(run, search);
        }
        true
    }
}

/// Bookkeeping of the modes.
#[derive(Debug, Default)]
pub(crate) struct TrainingFlow {
    active: Option<Active>,
    automove: Option<Duration>,
    generation: u64,
    tag_seq: u64,
    /// Searches of the modes that are not the one in flight (replaced or
    /// cancelled): what they report is dropped (D853).
    stale: Vec<(RunId, SearchId)>,
}

/// What the state looked like before a game message.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GameBefore {
    cursor: NodeId,
    /// The message plays a move.
    plays: bool,
}

/// The automove as a [`TrainingHook`] (the first of the Scid's order).
struct AutomoveHook;

impl TrainingHook for AutomoveHook {
    fn after_user_move(
        &mut self,
        state: &mut AppState,
        effects: &mut Vec<Effect>,
    ) -> TrainingOutcome {
        if state.automove_after_user_move(effects) {
            TrainingOutcome::Handled
        } else {
            TrainingOutcome::Continue
        }
    }
}

impl AppState {
    // ---- what the front-end reads ----

    /// What the engine of `run` is doing besides analysing the board. The
    /// threat shows on every section (all of them analyse the position with
    /// the null move).
    pub fn engine_mode(&self, run: RunId) -> EngineMode {
        let Some(active) = self.flow.training.active.as_ref() else {
            return EngineMode::Idle;
        };
        match active.kind {
            Kind::Threat { .. } => EngineMode::Threat,
            _ if active.run != run => EngineMode::Idle,
            Kind::ShootOut { .. } => EngineMode::ShootOut {
                generation: active.generation,
            },
            Kind::Automove {
                state, thinking_ms, ..
            } => EngineMode::Automove {
                generation: active.generation,
                thinking_ms,
                state,
            },
        }
    }

    /// Whether the position shown is the threat analysis (trial mode after
    /// a null move).
    pub fn threat_active(&self) -> bool {
        self.flow
            .training
            .active
            .as_ref()
            .is_some_and(|a| matches!(a.kind, Kind::Threat { .. }))
    }

    /// The thinking time of the automove (the default of its dialog).
    pub fn automove_time(&self) -> Duration {
        self.flow.training.automove.unwrap_or(DEFAULT_AUTOMOVE)
    }

    /// Which of the three buttons of the section of `run` can be used now.
    /// A button of a mode that is on stays enabled: it stops the mode.
    pub fn mode_availability(&self, run: RunId) -> ModeAvailability {
        let Some(r) = self.engine_runs.get(run) else {
            return ModeAvailability::default();
        };
        let free = r.state == EngineRunState::Running
            && !r.locked
            && !self.annotation_running()
            && !self.epd_uses(run)
            && !self.play_live()
            && !self.modal_open();
        let mode = self.engine_mode(run);
        let threat_on = mode == EngineMode::Threat;
        let shoot_on = matches!(mode, EngineMode::ShootOut { .. });
        let auto_on = matches!(mode, EngineMode::Automove { .. });
        let trial = self.session.in_trial();
        // What the Scid disables while a mode runs (D852): the shoot-out
        // takes the training and the threat away, the automove the shoot-out,
        // the threat the shoot-out and the training. The buttons of a mode
        // that is on stay enabled: they stop it.
        let other = self.flow.training.active.as_ref().map(Active::mode_kind);
        let shoot_other = other == Some(ModeKind::ShootOut);
        let auto_other = other == Some(ModeKind::Automove);
        let threat_other = other == Some(ModeKind::Threat);
        ModeAvailability {
            threat: free
                && (threat_on || (!trial && !shoot_other && !self.session.position().is_check())),
            finish_game: shoot_on || (free && !trial && !auto_other && !threat_other),
            training: auto_on || (free && !trial && !shoot_other && !threat_other),
        }
    }

    /// Whether `run` is the engine of a shoot-out or an automove: its
    /// MultiPV, exclusion and lock are out of reach meanwhile (the buttons
    /// of the Scid are disabled, `analysis.tcl` 3239; D852).
    pub fn engine_commandeered(&self, run: RunId) -> bool {
        self.training_uses(run)
    }

    /// Whether a shoot-out is playing the game (it holds an undo group).
    pub(crate) fn shootout_running(&self) -> bool {
        self.flow
            .training
            .active
            .as_ref()
            .is_some_and(|a| matches!(a.kind, Kind::ShootOut { .. }))
    }

    /// Refuses an edit that does not go through the game messages (the
    /// analysis' "add" buttons, the tags dialog) while the shoot-out plays:
    /// it would fall into the undo group of the engine's game (D858).
    /// Replacing or closing the game is not refused: it ends the shoot-out
    /// (D847). Sets the notice; returns whether it was refused.
    pub(crate) fn refuse_shootout_edit(&mut self) -> bool {
        if self.shootout_running() {
            self.mode_notice(ModeNotice::ShootOutBusy);
            true
        } else {
            false
        }
    }

    /// Whether a shoot-out or an automove commands the game.
    pub(crate) fn training_playing(&self) -> bool {
        self.flow
            .training
            .active
            .as_ref()
            .is_some_and(Active::plays)
    }

    /// Whether `run` is the engine of a shoot-out or an automove: the board's
    /// analysis does not drive it meanwhile.
    pub(crate) fn training_uses(&self, run: RunId) -> bool {
        // The engine of a game against an engine is not the analysis's
        // either (M8-1, D1241): every guard of the modes covers it too.
        self.play_uses(run)
            || self.epd_uses(run)
            || self
                .flow
                .training
                .active
                .as_ref()
                .is_some_and(|a| a.plays() && a.run == run)
    }

    // ---- messages ----

    pub(crate) fn mode_message(&mut self, message: ModeMessage, effects: &mut Vec<Effect>) {
        match message {
            ModeMessage::ToggleThreat(run) => self.threat_toggle(run, effects),
            ModeMessage::ToggleFinishGame(run) => self.shootout_toggle(run, effects),
            ModeMessage::ToggleAutomove(run) => self.automove_toggle(run, effects),
            ModeMessage::SetAutomoveTime(_, time) => {
                let (low, high) = (*AUTOMOVE_RANGE.start(), *AUTOMOVE_RANGE.end());
                self.flow.training.automove = Some(time.clamp(low, high));
            }
            ModeMessage::StopAll => {
                self.training_stop_by_user(effects);
            }
            ModeMessage::ModeTick { run, generation } => self.mode_tick(run, generation, effects),
        }
    }

    /// `Esc`: stops a shoot-out or an automove. Returns whether one was on.
    pub(crate) fn training_stop_by_user(&mut self, effects: &mut Vec<Effect>) -> bool {
        if !self.training_playing() {
            return false;
        }
        self.mode_stop(StopReason::UserStopped, effects);
        true
    }

    fn mode_notice(&mut self, notice: ModeNotice) {
        self.notice = Some(AppNotice::Mode(notice));
    }

    // ---- starting ----

    /// The checks every mode makes before it starts. `false`: refused (the
    /// notice, if any, is set).
    fn mode_can_start(&mut self, run: RunId) -> bool {
        let running = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if !running || self.modal_open() || self.session.prompt().is_some() {
            return false;
        }
        // The game against an engine owns the game and its engine (M8-1).
        if self.play_live() {
            self.play_busy();
            return false;
        }
        if self.annotation_running() {
            self.notice = Some(AppNotice::Annotate(crate::annotate::AnnotateNotice::Busy));
            return false;
        }
        // A training session or the calculation exercise holds the game of
        // the user aside: it ends first and the game comes back (D2358).
        self.tactics_halt();
        // A locked analysis refuses the buttons that act on the game.
        self.edit_allowed(run)
    }

    /// Turns off what a mode being started excludes: another mode and the
    /// autoplay (D495). Says so when something was on.
    fn mode_clear_others(&mut self, effects: &mut Vec<Effect>) {
        let mut conflict = false;
        if let Some(active) = self.flow.training.active.as_ref() {
            conflict = true;
            if active.plays() {
                self.mode_stop(StopReason::UserStopped, effects);
            } else {
                self.threat_leave(effects);
            }
        }
        if self.session.autoplay().running {
            conflict = true;
            effects.extend(self.session.update(GameMessage::StopAutoplay));
        }
        if conflict {
            self.mode_notice(ModeNotice::Conflict);
        }
    }

    /// The threat is a trial of its own: it goes away (and says so) before
    /// the user's trial is looked at (D854).
    fn threat_yields(&mut self, effects: &mut Vec<Effect>) {
        if self.threat_active() {
            self.threat_leave(effects);
            self.mode_notice(ModeNotice::Conflict);
        }
    }

    /// Takes the engine of `run` away from the board's analysis.
    fn mode_commandeer(&mut self, run: RunId) -> bool {
        let Some(r) = self.engine_runs.get_mut(run) else {
            return false;
        };
        let was = r.analyzing;
        r.analyzing = false;
        r.sent = None;
        was
    }

    fn mode_see(&mut self) {
        let seen = Seen {
            game: self.session.generation(),
            cursor: self.session.cursor(),
            changes: self.session.changes(),
        };
        if let Some(active) = self.flow.training.active.as_mut() {
            active.seen = seen;
        }
    }

    fn next_generation(&mut self) -> u64 {
        self.flow.training.generation = self.flow.training.generation.wrapping_add(1);
        self.flow.training.generation
    }

    fn start_active(&mut self, run: RunId, kind: Kind, restore_analyzing: bool) {
        let generation = self.next_generation();
        self.flow.training.active = Some(Active {
            run,
            kind,
            generation,
            restore_analyzing,
            seen: Seen {
                game: self.session.generation(),
                cursor: self.session.cursor(),
                changes: self.session.changes(),
            },
            inflight: None,
        });
    }

    // ---- threat ----

    fn threat_toggle(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if self.threat_active() {
            return self.threat_leave(effects);
        }
        if !self.mode_can_start(run) {
            return;
        }
        if self.session.in_trial() {
            return self.mode_notice(ModeNotice::TrialMode);
        }
        self.mode_clear_others(effects);
        // Checked here: entering trial mode first would leave it on when
        // the null move is refused (`main.tcl` 1326-1328).
        if self.session.position().is_check() {
            return self.mode_notice(ModeNotice::NullMoveInCheck);
        }
        let before = self.session.cursor();
        effects.extend(
            self.session
                .update(GameMessage::EnterTrial { null_move: true }),
        );
        self.take_session_analysis_notice();
        if !self.session.in_trial() {
            return;
        }
        if self.session.cursor() == before {
            // The null move did not go in: no threat to show.
            effects.extend(self.session.update(GameMessage::ExitTrial));
            return self.mode_notice(ModeNotice::NullMoveInCheck);
        }
        let node = self.session.cursor();
        let mut was = true;
        if let Some(r) = self.engine_runs.get_mut(run) {
            // The engine analyses the position with the null move; what it
            // did before comes back when the threat ends (D857).
            was = r.analyzing;
            r.analyzing = true;
        }
        self.start_active(run, Kind::Threat { node }, was);
    }

    /// Leaves the threat: the position before it comes back (the next
    /// refresh sends it to the engines).
    fn threat_leave(&mut self, effects: &mut Vec<Effect>) {
        let active = self.flow.training.active.take();
        effects.extend(self.session.update(GameMessage::ExitTrial));
        self.take_session_analysis_notice();
        self.threat_release(active, effects);
    }

    /// Gives the engine of a threat that ended back to what it did before:
    /// an engine that was idle is stopped.
    fn threat_release(&mut self, active: Option<Active>, effects: &mut Vec<Effect>) {
        let Some(active) = active else {
            return;
        };
        let hub = self.engine_hub.clone();
        let Some(r) = self
            .engine_runs
            .get_mut(active.run)
            .filter(|r| r.state == EngineRunState::Running)
        else {
            return;
        };
        r.analyzing = active.restore_analyzing;
        r.sent = None;
        if !r.analyzing {
            effects.push(Effect::EngineCommand {
                run: active.run,
                command: Box::new(EngineCommand::Stop),
                hub,
            });
        }
    }

    // ---- shoot-out ----

    fn shootout_toggle(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if self.training_uses(run)
            && self
                .flow
                .training
                .active
                .as_ref()
                .is_some_and(|a| matches!(a.kind, Kind::ShootOut { .. }))
        {
            return self.mode_stop(StopReason::UserStopped, effects);
        }
        if !self.mode_can_start(run) {
            return;
        }
        self.threat_yields(effects);
        if self.session.in_trial() {
            return self.mode_notice(ModeNotice::TrialMode);
        }
        self.mode_clear_others(effects);
        let was = self.mode_commandeer(run);
        // Not at the end of the line: the moves go to a new variation
        // (`sc_var create`, `analysis.tcl` 3234-3236).
        let at_end = self
            .session
            .game()
            .children(self.session.cursor())
            .map_or(true, <[NodeId]>::is_empty);
        if !at_end {
            effects.extend(self.session.update(GameMessage::StartVariation));
        }
        // One undo point for the whole game played by the engine.
        self.session.begin_edit_group();
        self.start_active(run, Kind::ShootOut { retried: false }, was);
        self.shootout_step(effects);
    }

    /// Asks the engine for the next move, or ends the game when it is over.
    fn shootout_step(&mut self, effects: &mut Vec<Effect>) {
        let Some((position, history)) = self.cursor_history() else {
            return self.mode_stop(StopReason::GameChanged, effects);
        };
        if let Some(reason) = terminal_reason(&position, &history) {
            return self.mode_stop(reason, effects);
        }
        // The same delay as the autoplay (`analysis.tcl` 3247, 3264).
        let time = self.session.autoplay().delay;
        self.mode_search(time, effects);
    }

    // ---- automove ----

    fn automove_toggle(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let on =
            self.flow.training.active.as_ref().is_some_and(|a| {
                a.plays() && a.run == run && matches!(a.kind, Kind::Automove { .. })
            });
        if on {
            return self.mode_stop(StopReason::UserStopped, effects);
        }
        if !self.mode_can_start(run) {
            return;
        }
        self.threat_yields(effects);
        if self.session.in_trial() {
            return self.mode_notice(ModeNotice::TrialMode);
        }
        self.mode_clear_others(effects);
        let was = self.mode_commandeer(run);
        let kind = Kind::Automove {
            state: AutomoveState::EngineThinking,
            attempts: 0,
            thinking_ms: 0,
        };
        self.start_active(run, kind, was);
        // Like the Scid, the engine answers at once: the side to move is
        // its side (`toggleAutomove` ends in `automove`, 4294-4296).
        self.automove_cycle(effects);
    }

    /// The thinking time in milliseconds, for the state of the section.
    fn automove_ms(&self) -> u32 {
        u32::try_from(self.automove_time().as_millis()).unwrap_or(u32::MAX)
    }

    /// Starts one move of the engine: ends the game if it is over, else the
    /// engine thinks.
    fn automove_cycle(&mut self, effects: &mut Vec<Effect>) {
        let generation = self.next_generation();
        let ms = self.automove_ms();
        self.mode_see();
        let Some((position, history)) = self.cursor_history() else {
            return self.mode_stop(StopReason::GameChanged, effects);
        };
        if let Some(reason) = terminal_reason(&position, &history) {
            return self.mode_stop(reason, effects);
        }
        if let Some(active) = self.flow.training.active.as_mut() {
            active.generation = generation;
            active.kind = Kind::Automove {
                state: AutomoveState::EngineThinking,
                attempts: 0,
                thinking_ms: ms,
            };
        }
        self.mode_search(Duration::from_millis(u64::from(ms)), effects);
    }

    /// The user played a move: the engine answers (D493, D494). Returns
    /// whether the automove was on.
    fn automove_after_user_move(&mut self, effects: &mut Vec<Effect>) -> bool {
        let on = self
            .flow
            .training
            .active
            .as_ref()
            .is_some_and(|a| matches!(a.kind, Kind::Automove { .. }));
        if !on {
            return false;
        }
        // A search for an older position is replaced by the new one.
        self.flow.training.drop_inflight();
        self.automove_cycle(effects);
        true
    }

    /// The timer of a retry fired.
    fn mode_tick(&mut self, run: RunId, generation: u64, effects: &mut Vec<Effect>) {
        // The watchdog of the search in flight: the engine never answered.
        let stalled = self.flow.training.active.as_ref().is_some_and(|a| {
            a.run == run && a.inflight.as_ref().is_some_and(|f| f.watch == generation)
        });
        if stalled {
            return self.mode_stop(StopReason::EngineError, effects);
        }
        let due = self.flow.training.active.as_ref().is_some_and(|a| {
            a.run == run
                && a.generation == generation
                && a.inflight.is_none()
                && matches!(
                    a.kind,
                    Kind::Automove {
                        state: AutomoveState::EngineThinking,
                        ..
                    }
                )
        });
        if !due {
            return;
        }
        let ms = self.automove_ms();
        self.mode_search(Duration::from_millis(u64::from(ms)), effects);
    }

    // ---- the searches ----

    /// The positions from the start of the game to the cursor: the position
    /// at the cursor and the hashes of the ones before it.
    fn cursor_history(&self) -> Option<(Position, Vec<ZobristHash>)> {
        let game = self.session.game();
        let path = game.path_to(self.session.cursor()).ok()?;
        let mut position = game.start_position().clone();
        let mut history = Vec::with_capacity(path.len());
        for node in path {
            history.push(position.zobrist());
            position.play(game.mv(node).ok()?).ok()?;
        }
        Some((position, history))
    }

    /// Sends a search of the position at the cursor that lasts `time`.
    fn mode_search(&mut self, time: Duration, effects: &mut Vec<Effect>) {
        let Some(run) = self.flow.training.active.as_ref().map(|a| a.run) else {
            return;
        };
        let Some(target) = self.analysis_target() else {
            return self.mode_stop(StopReason::GameChanged, effects);
        };
        self.flow.training.tag_seq = self.flow.training.tag_seq.wrapping_add(1);
        let tag = TAG_BASE | (self.flow.training.tag_seq & 0x0000_FFFF_FFFF_FFFF);
        let mut request = SearchRequest::infinite(mychess_engine::SearchPosition {
            start: target.start.clone(),
            moves: target.moves.clone(),
        });
        request.limits.infinite = false;
        request.limits.movetime = Some(time);
        request.tag = tag;
        effects.push(Effect::EngineCommand {
            run,
            command: Box::new(EngineCommand::Search(request)),
            hub: self.engine_hub.clone(),
        });
        // The watchdog: a refused or lost command, or a mute engine, must
        // not leave the mode "thinking" for ever (D855).
        let watch = self.next_generation();
        effects.push(Effect::ScheduleModeTick {
            run,
            generation: watch,
            delay: time.saturating_add(SEARCH_STALL_GRACE),
        });
        if let Some(active) = self.flow.training.active.as_mut() {
            active.inflight = Some(InFlight {
                tag,
                search: None,
                illegal: false,
                watch,
            });
        }
    }

    /// The command of the search was refused, lost or never delivered: it
    /// is dealt with like an unplayable answer (the table of D845).
    fn mode_search_lost(&mut self, effects: &mut Vec<Effect>) {
        let shoot_out = match self.flow.training.active.as_mut() {
            Some(active) => {
                active.inflight = None;
                matches!(active.kind, Kind::ShootOut { .. })
            }
            None => return,
        };
        if shoot_out {
            self.shootout_answer(None, true, false, effects);
        } else {
            self.automove_answer(true, false, effects);
        }
    }

    /// Whether a search of the mode of `run` was sent and not yet seen.
    fn awaiting_start(&self, run: RunId) -> bool {
        self.flow.training.active.as_ref().is_some_and(|a| {
            a.plays() && a.run == run && a.inflight.as_ref().is_some_and(|f| f.search.is_none())
        })
    }

    /// A command to the engine failed (refused, queue full).
    pub(crate) fn training_command_failed(
        &mut self,
        run: RunId,
        kind: CommandKind,
        effects: &mut Vec<Effect>,
    ) {
        if kind == CommandKind::Search && self.awaiting_start(run) {
            self.mode_search_lost(effects);
        }
    }

    /// The engine of `run` could not take a command (it is gone).
    pub(crate) fn training_command_gone(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if self.awaiting_start(run) {
            self.mode_search_lost(effects);
        }
    }

    /// Gives an event of an engine to the mode. Returns whether the mode
    /// took it (then it is not for the analysis window).
    pub(crate) fn training_event(
        &mut self,
        run: RunId,
        event: &EngineEvent,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let training = &mut self.flow.training;
        // The label of a search of the modes that is not the one in flight
        // (an older one, or one of a mode that ended): it and what it
        // reports are dropped, they are not the analysis window's (D853).
        if let EngineEvent::SearchTagged { search, tag } = event
            && is_mode_tag(*tag)
            && !training
                .active
                .as_ref()
                .is_some_and(|a| a.run == run && a.inflight.as_ref().is_some_and(|f| f.tag == *tag))
        {
            training.note_stale(run, *search);
            return true;
        }
        let reported = match event {
            EngineEvent::SearchStarted(id) => Some(*id),
            EngineEvent::Info(info) => Some(info.search),
            EngineEvent::BestMove(best) => Some(best.search),
            _ => None,
        };
        if let Some(id) = reported
            && let Some(at) = training.stale.iter().position(|e| *e == (run, id))
        {
            if matches!(event, EngineEvent::BestMove(_)) {
                training.stale.remove(at);
            }
            return true;
        }
        let Some(active) = training.active.as_mut() else {
            return false;
        };
        if active.run != run || !active.plays() {
            return false;
        }
        let Some(flight) = active.inflight.as_mut() else {
            return false;
        };
        match event {
            EngineEvent::SearchTagged { search, tag } if *tag == flight.tag => {
                flight.search = Some(*search);
                true
            }
            // An engine whose label was lost: the search that starts next
            // is ours.
            EngineEvent::SearchStarted(id)
                if flight.search.is_none() || flight.search == Some(*id) =>
            {
                flight.search.get_or_insert(*id);
                true
            }
            EngineEvent::Info(info) if flight.search == Some(info.search) => true,
            // The warning names no search: it is ours only once ours began.
            EngineEvent::Warning(EngineWarning::IllegalBestMove(_)) => {
                if flight.search.is_some() {
                    flight.illegal = true;
                }
                false
            }
            // The engine refused the position or the `go`: no search starts.
            EngineEvent::Warning(EngineWarning::CommandRefused { command, .. })
                if flight.search.is_none() && matches!(*command, "go" | "position") =>
            {
                self.mode_search_lost(effects);
                true
            }
            EngineEvent::BestMove(best) if flight.search == Some(best.search) => {
                let illegal = flight.illegal;
                active.inflight = None;
                self.training_best_move(best.mv, illegal, effects);
                true
            }
            _ => false,
        }
    }

    /// The engine answered.
    fn training_best_move(&mut self, mv: Option<Move>, illegal: bool, effects: &mut Vec<Effect>) {
        let shoot_out = self
            .flow
            .training
            .active
            .as_ref()
            .is_some_and(|a| matches!(a.kind, Kind::ShootOut { .. }));
        let played = mv.is_some_and(|mv| self.mode_play(mv, effects));
        if shoot_out {
            self.shootout_answer(mv, illegal, played, effects);
        } else {
            self.automove_answer(mv.is_some() || illegal, played, effects);
        }
    }

    fn shootout_answer(
        &mut self,
        mv: Option<Move>,
        illegal: bool,
        played: bool,
        effects: &mut Vec<Effect>,
    ) {
        if played {
            if let Some(Active {
                kind: Kind::ShootOut { retried },
                ..
            }) = self.flow.training.active.as_mut()
            {
                *retried = false;
            }
            return self.shootout_step(effects);
        }
        // No move at all (`bestmove (none)`): the game ends. A move that
        // could not be played: once more, then the engine is failing.
        if mv.is_none() && !illegal {
            return self.mode_stop(StopReason::NoMove, effects);
        }
        let again = match self.flow.training.active.as_mut() {
            Some(Active {
                kind: Kind::ShootOut { retried },
                ..
            }) if !*retried => {
                *retried = true;
                true
            }
            _ => false,
        };
        if again {
            self.shootout_step(effects);
        } else {
            self.mode_stop(StopReason::EngineError, effects);
        }
    }

    fn automove_answer(&mut self, misbehaved: bool, played: bool, effects: &mut Vec<Effect>) {
        if played {
            if let Some(Active {
                kind: Kind::Automove {
                    state, attempts, ..
                },
                ..
            }) = self.flow.training.active.as_mut()
            {
                *state = AutomoveState::UserToMove;
                *attempts = 0;
            }
            // The engine's move may have ended the game.
            let reason = self
                .cursor_history()
                .and_then(|(position, history)| terminal_reason(&position, &history));
            if let Some(reason) = reason {
                self.mode_stop(reason, effects);
            }
            return;
        }
        // Try again in a second, a few times (`analysis.tcl` 4324).
        let tries = match self.flow.training.active.as_mut() {
            Some(Active {
                kind: Kind::Automove { attempts, .. },
                ..
            }) => {
                *attempts = attempts.saturating_add(1);
                *attempts
            }
            _ => return,
        };
        if tries > AUTOMOVE_RETRIES {
            let reason = if misbehaved {
                StopReason::EngineError
            } else {
                StopReason::NoMove
            };
            return self.mode_stop(reason, effects);
        }
        let generation = self.next_generation();
        if let Some(active) = self.flow.training.active.as_mut() {
            active.generation = generation;
            effects.push(Effect::ScheduleModeTick {
                run: active.run,
                generation,
                delay: AUTOMOVE_RETRY_DELAY,
            });
        }
    }

    /// Plays the move of the engine in the game shown. `false`: it was not
    /// played.
    fn mode_play(&mut self, mv: Move, effects: &mut Vec<Effect>) -> bool {
        // A question of the user ("replace?") is not the engine's to answer:
        // the move waits for the retry (D857).
        if self.modal_open()
            || self.session.prompt().is_some()
            || !self.session.position().is_legal(mv)
        {
            return false;
        }
        let before = self.session.cursor();
        // A variation, never a question: the engine does not wait for an
        // answer (D496).
        effects.extend(self.session.update(GameMessage::PlayMove {
            mv,
            options: PlayOptions {
                force_variation: true,
            },
        }));
        self.take_session_analysis_notice();
        if self.session.prompt().is_some() {
            effects.extend(self.session.update(GameMessage::DismissPrompt));
        }
        let cursor = self.session.cursor();
        let moved =
            cursor != before && self.session.game().parent(cursor).ok().flatten() == Some(before);
        if moved {
            self.mode_see();
        }
        moved
    }

    // ---- the user moves the game ----

    /// Autoplay is refused while the engine plays (D495).
    pub(crate) fn training_refuses_autoplay(&mut self, message: &GameMessage) -> bool {
        let refused = self.training_playing()
            && matches!(
                message,
                GameMessage::StartAutoplay | GameMessage::ToggleAutoplay
            );
        if refused {
            self.mode_notice(ModeNotice::Conflict);
        }
        refused
    }

    /// An edit of the user ends the shoot-out *before* it reaches the
    /// session: the engine's undo group closes and the edit leaves an undo
    /// point of its own (D860). Navigation alone does not need it.
    pub(crate) fn training_stop_before_edit(
        &mut self,
        message: &GameMessage,
        effects: &mut Vec<Effect>,
    ) {
        let navigation = matches!(
            message,
            GameMessage::Navigate(_)
                | GameMessage::OpenVariationChooser
                | GameMessage::ChooserMove(_)
                | GameMessage::ChooserConfirm(_)
                | GameMessage::CloseVariationChooser
                | GameMessage::SetAutoplayDelay(_)
                | GameMessage::AutoplayTick { .. }
                | GameMessage::StopAutoplay
                | GameMessage::DismissPrompt
                | GameMessage::MoveEntryChar(_)
                | GameMessage::MoveEntryTyped(_)
                | GameMessage::MoveEntryBackspace
                | GameMessage::MoveEntryClear
        );
        if !navigation && self.shootout_running() {
            self.mode_stop(StopReason::GameChanged, effects);
        }
    }

    pub(crate) fn training_before_game_message(&self, message: &GameMessage) -> GameBefore {
        GameBefore {
            cursor: self.session.cursor(),
            plays: matches!(
                message,
                GameMessage::PlayMove { .. }
                    | GameMessage::PlaySquares { .. }
                    | GameMessage::PlayNullMove
                    | GameMessage::AnswerReplace(_)
                    | GameMessage::AnswerPromotion(_)
            ),
        }
    }

    /// After a game message: a move of the user goes to the hooks.
    pub(crate) fn training_after_game_message(
        &mut self,
        before: GameBefore,
        effects: &mut Vec<Effect>,
    ) {
        if !before.plays {
            return;
        }
        let cursor = self.session.cursor();
        let moved = cursor != before.cursor
            && self.session.game().parent(cursor).ok().flatten() == Some(before.cursor);
        if !moved {
            return;
        }
        let automove = self.training_playing()
            && !self.session.in_trial()
            && self
                .flow
                .training
                .active
                .as_ref()
                .is_some_and(|a| a.seen.game == self.session.generation());
        let mut auto = AutomoveHook;
        let mut tree = super::tree_train_flow::TreeTrainingHook;
        let mut hooks: Vec<&mut dyn TrainingHook> = Vec::new();
        if automove {
            hooks.push(&mut auto);
        }
        if self.tree.training {
            hooks.push(&mut tree);
        }
        run_hooks(&mut hooks, self, effects);
    }

    // ---- the end ----

    /// Ends the shoot-out or the automove: the engine is put back as it
    /// was and the reason is reported (except the user's own stop).
    fn mode_stop(&mut self, reason: StopReason, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.training.active.take() else {
            return;
        };
        // The search in flight is remembered: its late reports are dropped.
        if let Some(search) = active.inflight.as_ref().and_then(|f| f.search) {
            self.flow.training.note_stale(active.run, search);
        }
        if matches!(active.kind, Kind::ShootOut { .. }) {
            self.session.end_edit_group();
        }
        let alive = self
            .engine_runs
            .get(active.run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if alive {
            if active.inflight.is_some() {
                effects.push(Effect::EngineCommand {
                    run: active.run,
                    command: Box::new(EngineCommand::Stop),
                    hub: self.engine_hub.clone(),
                });
            }
            if let Some(r) = self.engine_runs.get_mut(active.run) {
                // The board's analysis comes back: the next refresh sends it.
                r.analyzing = active.restore_analyzing;
                r.sent = None;
            }
        }
        if reason != StopReason::UserStopped {
            let kind = active.mode_kind();
            self.mode_notice(ModeNotice::Ended { kind, reason });
        }
    }

    /// Called at the end of every update: the mode ends, or the pending
    /// move of the engine is dropped, when someone else moves the game.
    pub(crate) fn training_guard(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.training.active.as_ref() else {
            return;
        };
        let seen = active.seen;
        let current = Seen {
            game: self.session.generation(),
            cursor: self.session.cursor(),
            changes: self.session.changes(),
        };
        if let Kind::Threat { node } = active.kind {
            // The user left the null move or the trial: no threat any more.
            if !self.session.in_trial() || current.cursor != node || current.game != seen.game {
                let active = self.flow.training.active.take();
                self.threat_release(active, effects);
            }
            return;
        }
        let run = active.run;
        let alive = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if !alive {
            return self.mode_stop(StopReason::EngineError, effects);
        }
        if current.game != seen.game
            || self.session.in_trial()
            || self.modal_open()
            || self.annotation_running()
        {
            return self.mode_stop(StopReason::GameChanged, effects);
        }
        // A question of the user is not the engine's to answer: the user
        // stepped in (D861).
        if self.session.prompt().is_some() && matches!(active.kind, Kind::ShootOut { .. }) {
            return self.mode_stop(StopReason::GameChanged, effects);
        }
        if current == seen {
            return;
        }
        let shoot_out = matches!(active.kind, Kind::ShootOut { .. });
        if shoot_out {
            return self.mode_stop(StopReason::GameChanged, effects);
        }
        // Automove: the user navigated or edited. The pending move of the
        // engine no longer fits; the cycle starts again with the next move
        // of the user (rule 8).
        let generation = self.next_generation();
        let ms = self.automove_ms();
        let stop_search = self.flow.training.drop_inflight();
        if let Some(active) = self.flow.training.active.as_mut() {
            active.generation = generation;
            active.kind = Kind::Automove {
                state: AutomoveState::UserToMove,
                attempts: 0,
                thinking_ms: ms,
            };
            active.seen = current;
        }
        if stop_search {
            effects.push(Effect::EngineCommand {
                run,
                command: Box::new(EngineCommand::Stop),
                hub: self.engine_hub.clone(),
            });
        }
    }
}
