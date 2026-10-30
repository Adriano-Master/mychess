//! The coach in the game against an engine (spec M8-1d, D2416-D2430): the
//! glue between the pure [`CoachTracker`] and the auxiliary engine.
//!
//! The coach engine is another execution than the opponent's. This module
//! only ever looks at the events of that execution (`play_event` hands them
//! over before anything else), and the opponent's events never reach it
//! (D2416). Every search carries a tag of its own (`0x504D...`); late
//! answers of a search that was replaced or stopped are dropped by id
//! (D2422).

use std::time::{Duration, Instant};

use mychess_engine::{
    Bound, EngineCommand, EngineEvent, EngineWarning, OptionKind, OptionValue, Protocol, Score,
    SearchId, SearchLimits, SearchPosition, SearchRequest,
};

use super::{AppState, Restore, STALL_GRACE, Stage, previous_option};
use crate::effect::Effect;
use crate::engines::{EngineRunState, RunId};
use crate::play::{CoachOutput, CoachSearch, CoachTracker, CoachView, MoveRecord, PlayNotice};

/// The high bits of the tags of the searches of the coach.
const COACH_TAG_BASE: u64 = 0x504D_0000_0000_0000;

/// `Hash` given to the coach engine, in MB (D2424).
const COACH_HASH_MB: i64 = 64;

/// Whether `tag` labels a search of the coach.
pub(crate) fn is_coach_tag(tag: u64) -> bool {
    tag & 0xFFFF_0000_0000_0000 == COACH_TAG_BASE
}

/// The search in flight on the coach engine.
#[derive(Debug)]
struct Searching {
    /// The token the tracker gave it.
    token: u64,
    tag: u64,
    search: Option<SearchId>,
    started: Instant,
    last_event: Instant,
    allowed: Duration,
    ceiling: Duration,
    /// The last exact evaluation of the best line (White's point of view).
    score: Option<Score>,
    /// The depth that evaluation was made at (D2433).
    depth: Option<u32>,
}

/// The coach of a game.
#[derive(Debug)]
pub(crate) struct CoachSlot {
    run: RunId,
    name: String,
    tracker: CoachTracker,
    ready: bool,
    /// The engine is gone: the game goes on without the coach (D2423).
    lost: bool,
    /// The game is over: nothing more is judged and the engine was put back.
    closed: bool,
    /// What to put back in the engine; `None` once put back (or never
    /// prepared).
    restore: Option<Restore>,
    searching: Option<Searching>,
    /// The execution was started for the coach alone: it ends with the game
    /// (D2428, D2436).
    spawned: bool,
}

impl CoachSlot {
    /// The execution of the coach engine.
    pub(crate) fn run(&self) -> RunId {
        self.run
    }

    /// Whether a search of the coach is in flight.
    pub(crate) fn is_searching(&self) -> bool {
        self.searching.is_some()
    }

    /// What the panel shows (D2425).
    pub(crate) fn view(&self) -> CoachView {
        CoachView {
            engine: self.name.clone(),
            busy: self.tracker.busy(),
            lost: self.lost,
            numbers: self.tracker.config().numbers,
            last: self.tracker.last().cloned(),
        }
    }
}

impl AppState {
    /// Whether `run` is the execution of the coach of the game.
    pub(super) fn coach_owns(&self, run: RunId) -> bool {
        self.flow
            .play
            .active
            .as_ref()
            .and_then(|a| a.coach.as_ref())
            .is_some_and(|c| c.run == run)
    }

    fn coach_slot(&mut self) -> Option<&mut CoachSlot> {
        self.flow.play.active.as_mut()?.coach.as_mut()
    }

    // ---- start and end ----

    /// Creates the coach of the game that just began, if it asked for one
    /// and can have it (D2416, D2418).
    pub(super) fn coach_start(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_ref() else {
            return;
        };
        let Some(config) = active.config.coach else {
            return;
        };
        let Stage::Playing(controller) = &active.stage else {
            return;
        };
        let player = controller.player_color();
        let entry = self
            .engines
            .get(config.engine)
            .filter(|e| e.protocol == Protocol::Uci && e.id != active.config.engine)
            .cloned();
        let Some(entry) = entry else {
            return self.play_notice(PlayNotice::CoachUnavailable);
        };
        let game_run = active.run;
        // A run that was not there (nor starting) is the coach's alone.
        let spawned = !self
            .engine_runs
            .of_engine(entry.id)
            .is_some_and(|r| matches!(r.state, EngineRunState::Running | EngineRunState::Starting));
        let Some((run, ready)) = self.play_engine_run(&entry, effects) else {
            return self.play_notice(PlayNotice::CoachUnavailable);
        };
        if run == game_run {
            return self.play_notice(PlayNotice::CoachUnavailable);
        }
        let name = entry.name.clone();
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        if let Stage::Playing(controller) = &mut active.stage {
            controller.mark_game("Coach", &name);
        }
        active.coach = Some(CoachSlot {
            run,
            name,
            tracker: CoachTracker::new(config, player),
            ready: false,
            lost: false,
            closed: false,
            restore: None,
            searching: None,
            spawned,
        });
        if ready {
            self.coach_ready(effects);
        }
    }

    /// The coach engine is ready: it is set up (D2424) and the moves that
    /// waited are judged.
    pub(super) fn coach_ready(&mut self, effects: &mut Vec<Effect>) {
        let Some(slot) = self.coach_slot() else {
            return;
        };
        if slot.ready || slot.lost || slot.closed {
            return;
        }
        slot.ready = true;
        let run = slot.run;
        let restore = self.coach_prepare(run, effects);
        let Some(slot) = self.coach_slot() else {
            return;
        };
        slot.restore = Some(restore);
        let outputs = slot.tracker.set_ready(true);
        self.coach_outputs(outputs, effects);
    }

    /// Sets the coach engine up: new game, one line, one thread and a small
    /// hash so it does not compete with the opponent (D2424).
    fn coach_prepare(&mut self, run: RunId, effects: &mut Vec<Effect>) -> Restore {
        let mut restore = Restore::default();
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
            restore.analyzing = r.analyzing;
            r.analyzing = false;
            r.sent = None;
            r.seen_game = None;
        }
        self.play_send(run, EngineCommand::NewGame, effects);
        if let Some(entry) = entry.as_ref()
            && entry.multipv != 1
        {
            restore.multipv = Some(entry.multipv);
            self.play_send(run, EngineCommand::SetMultiPv(1), effects);
        }
        let saved: Vec<(String, String)> = entry.map(|e| e.options).unwrap_or_default();
        for (name, target) in [("Threads", 1), ("Hash", COACH_HASH_MB)] {
            let range = options
                .iter()
                .find_map(|o| match (&o.kind, o.name == name) {
                    (OptionKind::Spin { min, max, .. }, true) => Some((*min, *max)),
                    _ => None,
                });
            let Some((min, max)) = range else {
                continue;
            };
            if let Some(value) = previous_option(&options, &saved, name) {
                restore.strength.push((name.to_owned(), value));
            }
            self.play_send(
                run,
                EngineCommand::SetOption {
                    name: name.to_owned(),
                    value: OptionValue::Int(target.clamp(min, max)),
                },
                effects,
            );
        }
        restore
    }

    /// Puts the coach engine back as it was and drops what is pending. The
    /// last remark stays for the panel.
    pub(super) fn coach_restore(&mut self, effects: &mut Vec<Effect>) {
        let Some(slot) = self.coach_slot() else {
            return;
        };
        slot.ready = false;
        slot.closed = true;
        let (run, restore, searching) = (slot.run, slot.restore.take(), slot.searching.take());
        let spawned = std::mem::take(&mut slot.spawned);
        // The outputs are only a `Stop`, sent below; the tracker takes no
        // more moves.
        drop(slot.tracker.set_ready(false));
        if spawned {
            // An engine started only for the coach ends with the game; there
            // is nothing to put back in it (D2436).
            self.stop_run(run, effects);
            return;
        }
        let alive = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if !alive {
            return;
        }
        if let Some(s) = searching {
            self.coach_forget(run, &s);
            self.play_send(run, EngineCommand::Stop, effects);
        }
        let Some(restore) = restore else {
            return;
        };
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

    /// The coach engine died or was stopped: the game goes on (D2423).
    pub(super) fn coach_gone(&mut self, _effects: &mut Vec<Effect>) {
        let Some(slot) = self.coach_slot() else {
            return;
        };
        if slot.lost || slot.closed {
            return;
        }
        slot.lost = true;
        slot.ready = false;
        slot.restore = None;
        slot.searching = None;
        drop(slot.tracker.cancel());
        self.play_notice(PlayNotice::CoachLost);
    }

    fn coach_forget(&mut self, run: RunId, s: &Searching) {
        match s.search {
            Some(id) => self.play_remember(run, id),
            None => self.play_remember_tag(s.tag),
        }
    }

    /// The assistance was locked (an online game or its marker showed up
    /// meanwhile, D1330): the coach is help of the engines family, so it
    /// stops at once (D2432).
    pub(super) fn coach_lock_check(&mut self, effects: &mut Vec<Effect>) {
        if !self.assist_locked() {
            return;
        }
        if self.coach_slot().is_some_and(|s| !s.closed) {
            self.coach_restore(effects);
        }
    }

    // ---- the moves ----

    /// A move was made: if it is the player's, the coach judges it.
    pub(super) fn coach_moved(&mut self, record: &MoveRecord, effects: &mut Vec<Effect>) {
        self.coach_lock_check(effects);
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        let Stage::Playing(controller) = &active.stage else {
            return;
        };
        if record.mover != controller.player_color() || record.free {
            return;
        }
        let Some(slot) = active.coach.as_mut() else {
            return;
        };
        if slot.lost || slot.closed {
            return;
        }
        let moves = controller.played_moves();
        let before = usize::try_from(record.ply.saturating_sub(1)).unwrap_or(0);
        let Some(path) = moves.get(..before) else {
            return;
        };
        let outputs = slot.tracker.player_moved(
            controller.game().start_position(),
            path,
            record.mv,
            record.ply,
        );
        self.coach_outputs(outputs, effects);
    }

    fn coach_outputs(&mut self, outputs: Vec<CoachOutput>, effects: &mut Vec<Effect>) {
        for output in outputs {
            match output {
                CoachOutput::Search(search) => self.coach_search(&search, effects),
                CoachOutput::Stop => self.coach_stop(effects),
                // The tracker keeps the remark; the panel reads it.
                _ => {}
            }
        }
    }

    fn coach_search(&mut self, search: &CoachSearch, effects: &mut Vec<Effect>) {
        let now = self.play_now();
        let Some(slot) = self.coach_slot() else {
            return;
        };
        let config = *slot.tracker.config();
        let run = slot.run;
        self.flow.play.coach_tag_seq = self.flow.play.coach_tag_seq.wrapping_add(1);
        let tag = COACH_TAG_BASE | (self.flow.play.coach_tag_seq & 0x0000_FFFF_FFFF_FFFF);
        let Some(slot) = self.coach_slot() else {
            return;
        };
        let seconds = Duration::from_secs(u64::from(config.seconds));
        let old = slot.searching.replace(Searching {
            token: search.token,
            tag,
            search: None,
            started: now,
            last_event: now,
            allowed: seconds.saturating_add(STALL_GRACE),
            ceiling: seconds.saturating_mul(3).saturating_add(STALL_GRACE),
            score: None,
            depth: None,
        });
        if let Some(old) = old {
            self.coach_forget(run, &old);
        }
        let mut request = SearchRequest::infinite(SearchPosition {
            start: search.start.clone(),
            moves: search.moves.clone(),
        });
        request.limits = SearchLimits {
            depth: Some(config.depth),
            movetime: Some(seconds),
            ..SearchLimits::default()
        };
        request.search_moves = search.only.into_iter().collect();
        request.tag = tag;
        self.play_send(run, EngineCommand::Search(request), effects);
    }

    fn coach_stop(&mut self, effects: &mut Vec<Effect>) {
        let Some(slot) = self.coach_slot() else {
            return;
        };
        let (run, searching) = (slot.run, slot.searching.take());
        if let Some(s) = searching {
            self.coach_forget(run, &s);
            self.play_send(run, EngineCommand::Stop, effects);
        }
    }

    // ---- the engine's events ----

    /// An event of the coach execution. Returns whether it was taken.
    pub(super) fn coach_event(
        &mut self,
        run: RunId,
        event: &EngineEvent,
        effects: &mut Vec<Effect>,
    ) -> bool {
        self.coach_lock_check(effects);
        let now = self.play_now();
        if let EngineEvent::SearchTagged { search, tag } = event
            && is_coach_tag(*tag)
        {
            let current = self
                .coach_slot()
                .and_then(|s| s.searching.as_mut())
                .filter(|s| s.tag == *tag);
            if let Some(s) = current {
                s.search = Some(*search);
                s.last_event = now;
            } else {
                self.flow.play.stale_tags.retain(|t| t != tag);
                self.play_remember(run, *search);
            }
            return true;
        }
        if self.play_drop_stale(run, event) {
            return true;
        }
        let Some(slot) = self.coach_slot() else {
            return false;
        };
        match event {
            EngineEvent::SearchStarted(id) => match slot.searching.as_mut() {
                Some(s) if s.search.is_none() || s.search == Some(*id) => {
                    s.search = Some(*id);
                    s.last_event = now;
                    true
                }
                _ => false,
            },
            EngineEvent::Info(info) => match slot.searching.as_mut() {
                Some(s) if s.search == Some(info.search) => {
                    s.last_event = now;
                    if info.multipv <= 1
                        && let Some((score, bound)) = info.score
                        && (bound == Bound::Exact || s.score.is_none())
                    {
                        s.score = Some(score);
                        s.depth = info.depth;
                    }
                    true
                }
                _ => false,
            },
            EngineEvent::BestMove(best) => {
                let ours = slot
                    .searching
                    .as_ref()
                    .is_some_and(|s| s.search == Some(best.search));
                if ours && let Some(s) = slot.searching.take() {
                    let outputs = slot.tracker.search_done(s.token, best.mv, s.score, s.depth);
                    self.coach_outputs(outputs, effects);
                }
                ours
            }
            EngineEvent::Warning(EngineWarning::CommandRefused {
                command: "go" | "position",
                ..
            }) if slot.searching.as_ref().is_some_and(|s| s.search.is_none()) => {
                self.coach_search_failed(false, effects);
                false
            }
            _ => false,
        }
    }

    /// The search in flight failed (refused, silent, too long): the move is
    /// left unjudged and the next one goes on (D2422, D2423).
    fn coach_search_failed(&mut self, stop: bool, effects: &mut Vec<Effect>) {
        let Some(slot) = self.coach_slot() else {
            return;
        };
        let (run, searching) = (slot.run, slot.searching.take());
        let Some(s) = searching else {
            return;
        };
        let outputs = slot.tracker.search_done(s.token, None, None, None);
        self.coach_forget(run, &s);
        if stop {
            self.play_send(run, EngineCommand::Stop, effects);
        }
        self.coach_outputs(outputs, effects);
    }

    /// A search of the coach was refused or lost.
    pub(super) fn coach_command_failed(&mut self, effects: &mut Vec<Effect>) {
        let awaiting = self
            .coach_slot()
            .and_then(|s| s.searching.as_ref())
            .is_some_and(|s| s.search.is_none());
        if awaiting {
            self.coach_search_failed(false, effects);
        }
    }

    /// A tick: a search that went silent or ran too long is given up.
    pub(super) fn coach_tick(&mut self, at: Instant, effects: &mut Vec<Effect>) {
        self.coach_lock_check(effects);
        let overdue = self
            .coach_slot()
            .and_then(|s| s.searching.as_ref())
            .is_some_and(|s| {
                at.saturating_duration_since(s.last_event) > s.allowed
                    || at.saturating_duration_since(s.started) > s.ceiling
            });
        if overdue {
            self.coach_search_failed(true, effects);
        }
    }
}
