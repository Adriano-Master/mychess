//! The state machine of the tactics training (spec M8-4, D1913-D1936).
//!
//! Pure: [`Tactics`] never touches the editor, the clock or the disk. Every
//! method returns [`Out`] commands that the glue ([`flow`](super::flow)) runs:
//! effects, moves to put in the editor game, saving the store.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use mychess_core::{Move, Position, Square};

use super::config::{SourceKind, TacticsConfig, TacticsMode, TacticsOrder};
use super::judge::{JudgeJob, JudgeRule, Verdict, Wrong, quick_verdict};
use super::message::{TacticsEffect, TacticsNotice};
use super::opinion::{OpinionEngine, OpinionJob, OpinionVerdict};
use super::problem::Problem;
use super::source::{
    Cancel, FindOutcome, FindQuery, Found, GAMES_PER_REQUEST, OpenedSource, SourceError,
    SourceHandle, SourceStamp, coprime_step,
};
use super::stats::{FailReason, ModeStats, ProblemOutcome};
use super::store::{LoadedStore, TacticsStore};
use crate::bases::SharedSource;
use crate::error::{FileError, RequestId};

/// A request that waits this long (in ticks) is dropped (D1934).
pub const WATCHDOG_MS: u64 = 60_000;
/// The most that one tick adds to a clock (D1925).
pub const MAX_TICK_MS: u64 = 2_000;

/// Where the training is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TacticsPhase {
    /// No session.
    Closed,
    /// Reading the source.
    Opening,
    /// Looking for the next problem.
    Searching,
    /// The user is to move.
    Solving,
    /// The solver is judging a move.
    Judging,
    /// The problem was solved.
    Solved(ProblemOutcome),
    /// The problem was lost.
    Failed(FailReason),
    /// There is no problem left in the source.
    Exhausted,
}

impl TacticsPhase {
    /// Whether a session is on.
    pub fn is_live(self) -> bool {
        self != TacticsPhase::Closed
    }

    /// Whether the front-end must send ticks.
    pub fn wants_ticks(self) -> bool {
        matches!(
            self,
            TacticsPhase::Opening
                | TacticsPhase::Searching
                | TacticsPhase::Solving
                | TacticsPhase::Judging
        )
    }
}

/// What the last move of the user showed (D1910, D1913).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Feedback {
    /// Nothing yet.
    #[default]
    None,
    /// The last move was right and the problem goes on.
    Correct,
    /// The last move was wrong.
    Wrong(Wrong),
}

/// The squares of a hint (D1919).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    /// The piece to move.
    pub from: Square,
    /// Where it goes (second level).
    pub to: Option<Square>,
}

/// A command for the glue.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Out {
    /// Run an effect.
    Effect(TacticsEffect),
    /// Put the problem in the editor: a game that starts at this position,
    /// with the side to move at the bottom.
    ShowProblem(Box<Position>),
    /// Play the move at the end of the game of the editor.
    Play(Move),
    /// Add the moves to the main line, from the end of the game.
    AddLine(Vec<Move>),
    /// Write the store.
    SaveStore,
    /// The session ended: the previous game of the editor comes back.
    Closed,
    /// The session ended and the game that is in the editor stays (D2140).
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Open,
    Find,
    Judge,
    Opinion,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    request: RequestId,
    kind: PendingKind,
    waited_ms: u64,
}

#[derive(Debug, Clone)]
struct ActiveSource {
    handle: SourceHandle,
    key: String,
    len: usize,
    start: usize,
    step: usize,
    k: usize,
    stamp: Option<SourceStamp>,
}

/// The problem on the board.
#[derive(Debug, Clone)]
struct Active {
    problem: Problem,
    current: Position,
    /// The mate that is left (Mate in N).
    remaining: Option<u8>,
    /// The line from `current`.
    line: Vec<Move>,
    initial_remaining: Option<u8>,
    initial_line: Vec<Move>,
    attempts_used: u8,
    mistakes: u8,
    helped: bool,
    hint_level: u8,
    elapsed_ms: u64,
}

/// A read-only picture of the machine for the front-end.
#[derive(Debug, Clone, PartialEq)]
pub struct TacticsView {
    /// Where the training is.
    pub phase: TacticsPhase,
    /// The mode of the session (or of the dialog when closed).
    pub mode: TacticsMode,
    /// Whether the configuration dialog is open.
    pub dialog_open: bool,
    /// The configuration of the last start.
    pub config: TacticsConfig,
    /// The name of the problem.
    pub title: String,
    /// The mate that is left to find (Mate in N).
    pub mate_in: Option<u8>,
    /// What the last move showed.
    pub feedback: Feedback,
    /// A remark, if any.
    pub notice: Option<TacticsNotice>,
    /// Why the score is not being saved, for as long as that holds (D1954).
    pub store_issue: Option<TacticsNotice>,
    /// Attempts left on this problem.
    pub attempts_left: u8,
    /// Attempts allowed.
    pub attempts_max: u8,
    /// Seconds spent on this problem.
    pub elapsed_s: u32,
    /// The time limit in seconds, 0 for none.
    pub time_limit_s: u16,
    /// The hint to draw on the board.
    pub hint: Option<Hint>,
    /// The score of the mode.
    pub stats: ModeStats,
    /// Games looked at while searching.
    pub examined: usize,
    /// Problems of the current source already solved.
    pub solved_in_source: usize,
    /// The hint button works.
    pub can_hint: bool,
    /// "Show the solution" works.
    pub can_show_solution: bool,
    /// "Next problem" works.
    pub can_next: bool,
    /// "Try again" works.
    pub can_retry: bool,
    /// "Continue against the engine" works (D2140).
    pub can_continue: bool,
    /// "Best move" was asked to use an engine and there is none to ask
    /// (D2131): the panel says so for as long as the session lasts.
    pub opinion_missing: bool,
    /// Problems of mate beyond "Longest mate" met so far in this session
    /// (D2155), a lower bound.
    pub beyond_limit: u32,
}

/// The machine.
#[derive(Debug)]
pub struct Tactics {
    phase: TacticsPhase,
    dialog_open: bool,
    config: TacticsConfig,
    store: TacticsStore,
    store_loaded: bool,
    store_loading: bool,
    /// Why the file must not be written (newer version, or unreadable for a
    /// reason that may pass, D1954); shown for as long as it holds.
    store_issue: Option<TacticsNotice>,
    /// The score changed in this run (so a late read must not replace it).
    used: bool,
    source: Option<ActiveSource>,
    next_request: u64,
    pending: Option<Pending>,
    notice: Option<TacticsNotice>,
    examined: usize,
    scan_left: usize,
    /// Stops the search that is running on another thread (D1984).
    find_cancel: Option<Cancel>,
    active: Option<Active>,
    feedback: Feedback,
    last_tick: Option<Instant>,
    /// The seed of the random order of the session.
    seed: u64,
    /// The move being judged by the solver.
    judging: Option<Move>,
    /// The engine that gives the second opinion, when there is one (D2131).
    opinion_engine: Option<OpinionEngine>,
    /// Stops the question to the engine (D2133).
    opinion_cancel: Option<Cancel>,
    /// The mate problems beyond the longest mate asked for (D2155), by id.
    beyond_ids: HashSet<u64>,
}

impl Default for Tactics {
    fn default() -> Tactics {
        Tactics {
            phase: TacticsPhase::Closed,
            dialog_open: false,
            config: TacticsConfig::default(),
            store: TacticsStore::default(),
            store_loaded: false,
            store_loading: false,
            store_issue: None,
            used: false,
            source: None,
            next_request: 0,
            pending: None,
            notice: None,
            examined: 0,
            scan_left: 0,
            find_cancel: None,
            active: None,
            feedback: Feedback::None,
            last_tick: None,
            seed: 0,
            judging: None,
            opinion_engine: None,
            opinion_cancel: None,
            beyond_ids: HashSet::new(),
        }
    }
}

impl Tactics {
    /// A machine with nothing loaded.
    pub fn new() -> Tactics {
        Tactics::default()
    }

    // ---- reading ----

    /// Where the training is.
    pub fn phase(&self) -> TacticsPhase {
        self.phase
    }

    /// Whether a session is on.
    pub fn is_live(&self) -> bool {
        self.phase.is_live()
    }

    /// The position the problem on the board started from, while there is
    /// one (a tactical game may start from it, M8-7, D2459).
    pub fn problem_position(&self) -> Option<&Position> {
        self.active.as_ref().map(|a| &a.problem.position)
    }

    /// Whether the dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.dialog_open
    }

    /// The configuration of the last start (or of the file).
    pub fn config(&self) -> &TacticsConfig {
        &self.config
    }

    /// The remark waiting to be shown.
    pub fn notice(&self) -> Option<TacticsNotice> {
        self.notice
    }

    /// Sets the remark.
    pub(crate) fn set_notice(&mut self, notice: TacticsNotice) {
        self.notice = Some(notice);
    }

    /// Takes the remark.
    pub fn take_notice(&mut self) -> Option<TacticsNotice> {
        self.notice.take()
    }

    /// The user's moves are judged now.
    pub fn takes_moves(&self) -> bool {
        self.phase == TacticsPhase::Solving
    }

    /// The editor must not change: the problem is not over yet (D1930).
    pub fn locks_editor(&self) -> bool {
        matches!(
            self.phase,
            TacticsPhase::Opening
                | TacticsPhase::Searching
                | TacticsPhase::Solving
                | TacticsPhase::Judging
        )
    }

    /// The key of the source of the session.
    pub fn source_key(&self) -> Option<&str> {
        self.source.as_ref().map(|s| s.key.as_str())
    }

    /// The hint to draw (D1940).
    pub fn hint(&self) -> Option<Hint> {
        let active = self.active.as_ref()?;
        if active.hint_level == 0 || self.phase != TacticsPhase::Solving {
            return None;
        }
        let mv = active.line.first()?;
        Some(Hint {
            from: mv.from()?,
            to: (active.hint_level >= 2).then(|| mv.to()).flatten(),
        })
    }

    /// The picture for the front-end.
    pub fn view(&self) -> TacticsView {
        let mode = self.config.mode;
        let active = self.active.as_ref();
        let solving = self.phase == TacticsPhase::Solving;
        let ended = matches!(
            self.phase,
            TacticsPhase::Solved(_) | TacticsPhase::Failed(_)
        );
        TacticsView {
            phase: self.phase,
            mode,
            dialog_open: self.dialog_open,
            config: self.config.clone(),
            title: active.map(|a| a.problem.title.clone()).unwrap_or_default(),
            mate_in: active.and_then(|a| a.remaining),
            feedback: self.feedback,
            notice: self.notice,
            store_issue: self.store_issue,
            attempts_left: active.map_or(0, |a| {
                self.config.max_attempts.saturating_sub(a.attempts_used)
            }),
            attempts_max: self.config.max_attempts,
            elapsed_s: active.map_or(0, |a| {
                u32::try_from(a.elapsed_ms / 1000).unwrap_or(u32::MAX)
            }),
            time_limit_s: self.config.time_limit_s,
            hint: self.hint(),
            stats: *self.store.stats.of(mode),
            examined: self.examined,
            solved_in_source: self
                .source
                .as_ref()
                .map_or(0, |s| self.store.solved_count(&s.key)),
            can_hint: solving
                && self.config.hints
                && active.is_some_and(|a| !a.line.is_empty() && a.hint_level < 2),
            can_show_solution: solving,
            can_next: solving || ended,
            can_retry: matches!(self.phase, TacticsPhase::Failed(_)),
            can_continue: ended
                && active.is_some_and(|a| {
                    // A mate that was delivered leaves nothing to play.
                    !a.current.legal_moves().is_empty()
                }),
            opinion_missing: self.is_live()
                && self.config.mode == TacticsMode::BestMove
                && self.config.second_opinion
                && self.opinion_engine.is_none(),
            beyond_limit: u32::try_from(self.beyond_ids.len()).unwrap_or(u32::MAX),
        }
    }

    // ---- the store ----

    /// Whether the store was read or is being read.
    pub fn store_requested(&self) -> bool {
        self.store_loaded || self.store_loading
    }

    /// Marks the read as asked for; the glue sends the effect.
    pub fn begin_load(&mut self) {
        self.store_loading = true;
    }

    /// The read finished.
    pub fn store_loaded(&mut self, loaded: LoadedStore) {
        self.store_loading = false;
        self.store_loaded = true;
        if loaded.read_only {
            self.store_issue = Some(if loaded.newer {
                TacticsNotice::StoreNewer
            } else {
                TacticsNotice::StoreUnreadable
            });
        } else if loaded.unreadable {
            self.notice = Some(TacticsNotice::StoreUnreadable);
        }
        let live = self.is_live();
        self.store.merge_loaded(loaded.store, self.used || live);
        if !live {
            self.config = self.store.config.clone();
        }
    }

    /// The data to write.
    pub fn store(&self) -> &TacticsStore {
        &self.store
    }

    /// Whether `tactics.toml` must be left alone (D1954).
    pub fn store_read_only(&self) -> bool {
        self.store_issue.is_some()
    }

    /// The write finished.
    pub fn store_saved(&mut self, result: &Result<(), FileError>) {
        if result.is_err() {
            self.notice = Some(TacticsNotice::StoreNotSaved);
        }
    }

    // ---- the dialog ----

    /// Opens the dialog.
    pub fn open_dialog(&mut self) {
        self.dialog_open = true;
        self.notice = None;
    }

    /// Closes the dialog.
    pub fn close_dialog(&mut self) {
        self.dialog_open = false;
    }

    /// Clears the score of `mode` and the problems solved in `key` (D1927).
    pub fn reset(&mut self, mode: TacticsMode, key: Option<&str>) -> Vec<Out> {
        self.store.reset_stats(mode);
        if let Some(key) = key {
            self.store.reset_source(key);
        }
        self.used = true;
        vec![Out::SaveStore]
    }

    // ---- starting and ending ----

    fn request(&mut self, kind: PendingKind) -> RequestId {
        self.next_request = self.next_request.wrapping_add(1);
        let request = RequestId(self.next_request);
        self.pending = Some(Pending {
            request,
            kind,
            waited_ms: 0,
        });
        request
    }

    fn set_phase(&mut self, phase: TacticsPhase) {
        // A remark is about the phase it was made in (D1950); the ones that
        // explain why the session ended stay.
        if phase != TacticsPhase::Closed {
            self.notice = None;
        }
        self.phase = phase;
        self.last_tick = None;
    }

    /// Starts a session (D1928). `base` is the opened current base when the
    /// source asks for it; the glue builds it.
    pub fn start(
        &mut self,
        config: TacticsConfig,
        seed: u64,
        base: Option<OpenedSource>,
    ) -> Vec<Out> {
        // A session that is on is never restarted from here (D1949): the
        // glue refuses it, and this keeps the machine safe on its own.
        if self.is_live() {
            return Vec::new();
        }
        let config = config.sanitized();
        // Nothing changes until the source is known to exist (D1949), so a
        // refusal leaves the dialog and the state as they were.
        let missing = match config.source {
            SourceKind::File => config.file.is_none(),
            SourceKind::CurrentBase => base.is_none(),
        };
        if missing {
            self.notice = Some(TacticsNotice::NoSource);
            return Vec::new();
        }
        self.pending = None;
        self.notice = None;
        self.active = None;
        self.scan_left = 0;
        self.judging = None;
        self.beyond_ids.clear();
        let mut outs = Vec::new();
        self.store.config = config.clone();
        self.config = config;
        self.examined = 0;
        self.feedback = Feedback::None;
        self.dialog_open = false;
        self.seed = seed;
        self.source = None;
        if let (SourceKind::File, Some(path)) = (self.config.source, self.config.file.clone()) {
            let request = self.request(PendingKind::Open);
            self.set_phase(TacticsPhase::Opening);
            outs.push(Out::Effect(TacticsEffect::OpenFile { request, path }));
        } else if let Some(opened) = base {
            self.set_phase(TacticsPhase::Opening);
            outs.extend(self.source_opened(opened));
        }
        outs.push(Out::SaveStore);
        outs
    }

    fn source_opened(&mut self, opened: OpenedSource) -> Vec<Out> {
        let len = opened.handle.len();
        if len == 0 {
            self.notice = Some(TacticsNotice::NoGames);
            return self.end_session(true);
        }
        let (start, step) = match self.config.order {
            TacticsOrder::Sequential => (0, 1),
            TacticsOrder::Random => (
                usize::try_from(self.seed % u64::try_from(len).unwrap_or(1)).unwrap_or(0),
                coprime_step(self.seed, len),
            ),
        };
        self.source = Some(ActiveSource {
            handle: opened.handle,
            key: opened.key,
            len,
            start,
            step,
            k: 0,
            stamp: opened.stamp,
        });
        self.request_next()
    }

    /// The answer to [`TacticsEffect::OpenFile`].
    pub fn opened(
        &mut self,
        request: RequestId,
        result: Result<Box<OpenedSource>, SourceError>,
    ) -> Vec<Out> {
        if !self.takes_answer(request, PendingKind::Open) {
            return Vec::new();
        }
        self.pending = None;
        let opened = match result {
            Ok(opened) => opened,
            Err(error) => {
                self.notice = Some(match error {
                    SourceError::NeedsUpgrade => TacticsNotice::SourceNeedsUpgrade,
                    SourceError::Corrupt => TacticsNotice::SourceCorrupt,
                    SourceError::Unfinished => TacticsNotice::SourceUnfinished,
                    _ => TacticsNotice::SourceUnreadable,
                });
                return self.end_session(true);
            }
        };
        self.source_opened(*opened)
    }

    /// Tells the question to the engine to stop (D2133).
    fn cancel_opinion(&mut self) {
        if let Some(cancel) = self.opinion_cancel.take() {
            cancel.cancel();
        }
    }

    /// The engine to ask when a move differs from the one of the game; the
    /// glue gives it before a start, and only when the option is on and the
    /// help is not locked (D2131).
    pub fn set_opinion_engine(&mut self, engine: Option<OpinionEngine>) {
        self.opinion_engine = engine;
    }

    /// Tells the search on its thread to stop (D1984).
    fn cancel_find(&mut self) {
        if let Some(cancel) = self.find_cancel.take() {
            cancel.cancel();
        }
    }

    fn takes_answer(&self, request: RequestId, kind: PendingKind) -> bool {
        self.pending
            .is_some_and(|p| p.request == request && p.kind == kind)
    }

    /// Ends the session and asks the glue to put the previous game back;
    /// `reopen` brings the dialog back to show what went wrong.
    fn end_session(&mut self, reopen: bool) -> Vec<Out> {
        self.dialog_open = reopen;
        self.pending = None;
        self.active = None;
        self.source = None;
        self.scan_left = 0;
        self.judging = None;
        self.cancel_find();
        self.cancel_opinion();
        self.feedback = Feedback::None;
        self.set_phase(TacticsPhase::Closed);
        let mut outs = vec![Out::Closed];
        if self.used {
            outs.push(Out::SaveStore);
        }
        outs
    }

    /// The database of a session of the current base was reopened with the
    /// same games (a change of flags writes and reads the file again): the
    /// session reads from the new one, so the old one is released (D1953).
    pub fn rebase(&mut self, new: SharedSource) {
        if let Some(ActiveSource {
            handle: SourceHandle::Base { source, .. },
            ..
        }) = self.source.as_mut()
        {
            *source = new;
        }
    }

    /// Ends the session because of `notice`, bringing the dialog back to
    /// show it (D1953).
    pub fn abort(&mut self, notice: TacticsNotice) -> Vec<Out> {
        if !self.is_live() {
            return Vec::new();
        }
        let outs = self.end_session(true);
        self.notice = Some(notice);
        outs
    }

    /// Ends the session (the "Close" button, D1935).
    pub fn close(&mut self) -> Vec<Out> {
        self.dialog_open = false;
        if !self.is_live() {
            return Vec::new();
        }
        self.end_session(false)
    }

    // ---- looking for a problem ----

    fn request_next(&mut self) -> Vec<Out> {
        let Some(source) = self.source.as_ref() else {
            return self.end_session(true);
        };
        if self.scan_left == 0 {
            self.scan_left = source.len;
            self.examined = 0;
        }
        let cancel = Cancel::new();
        let query = FindQuery {
            config: Box::new(self.config.clone()),
            start: source.start,
            step: source.step,
            k: source.k,
            max_games: self.scan_left.clamp(1, GAMES_PER_REQUEST),
            solved: self.store.solved_ids(&source.key),
            stamp: source.stamp.clone(),
            cancel: cancel.clone(),
        };
        let handle = source.handle.clone();
        self.find_cancel = Some(cancel);
        let request = self.request(PendingKind::Find);
        self.set_phase(TacticsPhase::Searching);
        vec![Out::Effect(TacticsEffect::Find {
            request,
            source: handle,
            query: Box::new(query),
        })]
    }

    /// The answer to [`TacticsEffect::Find`].
    pub fn found(&mut self, request: RequestId, outcome: FindOutcome) -> Vec<Out> {
        if !self.takes_answer(request, PendingKind::Find) {
            return Vec::new();
        }
        self.pending = None;
        let FindOutcome {
            found,
            next_k,
            examined,
            source_changed,
            beyond_ids,
            ..
        } = outcome;
        self.beyond_ids.extend(beyond_ids);
        self.find_cancel = None;
        // The file of the database changed under the session (D1987).
        if source_changed {
            return self.abort(TacticsNotice::SourceChanged);
        }
        if let Some(source) = self.source.as_mut() {
            source.k = next_k;
        }
        self.examined = self.examined.saturating_add(examined);
        self.scan_left = self.scan_left.saturating_sub(examined);
        if let Some(found) = found {
            self.scan_left = 0;
            return self.present(*found);
        }
        // Nothing read at all (the work failed): do not loop on it.
        if examined == 0 || self.scan_left == 0 {
            self.scan_left = 0;
            self.set_phase(TacticsPhase::Exhausted);
            return Vec::new();
        }
        self.request_next()
    }

    fn present(&mut self, found: Found) -> Vec<Out> {
        let Found {
            problem,
            mate_moves,
            line,
        } = found;
        let position = problem.position.clone();
        self.store.stats.of_mut(self.config.mode).record_shown();
        self.used = true;
        self.active = Some(Active {
            current: position.clone(),
            problem,
            remaining: mate_moves,
            initial_remaining: mate_moves,
            initial_line: line.clone(),
            line,
            attempts_used: 0,
            mistakes: 0,
            helped: false,
            hint_level: 0,
            elapsed_ms: 0,
        });
        self.feedback = Feedback::None;
        self.set_phase(TacticsPhase::Solving);
        vec![Out::ShowProblem(Box::new(position)), Out::SaveStore]
    }

    // ---- solving ----

    /// The user played `mv` on the board.
    pub fn user_move(&mut self, mv: Move) -> Vec<Out> {
        if self.phase != TacticsPhase::Solving {
            return Vec::new();
        }
        self.notice = None;
        let Some(active) = self.active.as_ref() else {
            return Vec::new();
        };
        let rule = match self.config.mode {
            TacticsMode::BestMove => JudgeRule::Best {
                solution: active.line.clone(),
            },
            _ => JudgeRule::Mate {
                remaining: active.remaining.unwrap_or(1),
                accept_longer: self.config.accept_longer,
            },
        };
        let job = JudgeJob {
            position: active.current.clone(),
            mv,
            rule,
        };
        if let Some(verdict) = quick_verdict(&job) {
            return self.verdict(mv, verdict);
        }
        let request = self.request(PendingKind::Judge);
        self.judging = Some(mv);
        self.set_phase(TacticsPhase::Judging);
        vec![Out::Effect(TacticsEffect::Judge {
            request,
            job: Box::new(job),
        })]
    }

    /// The answer to [`TacticsEffect::Judge`].
    pub fn judged(&mut self, request: RequestId, verdict: Verdict) -> Vec<Out> {
        if !self.takes_answer(request, PendingKind::Judge) {
            return Vec::new();
        }
        self.pending = None;
        let Some(mv) = self.judging.take() else {
            self.set_phase(TacticsPhase::Solving);
            return Vec::new();
        };
        self.set_phase(TacticsPhase::Solving);
        self.verdict(mv, verdict)
    }

    fn verdict(&mut self, mv: Move, verdict: Verdict) -> Vec<Out> {
        let mut outs = Vec::new();
        match verdict {
            Verdict::Correct {
                mated,
                reply,
                remaining,
                line,
            } => {
                let Some(active) = self.active.as_mut() else {
                    return outs;
                };
                outs.push(Out::Play(mv));
                if let Ok(next) = active.current.after(mv) {
                    active.current = next;
                }
                active.hint_level = 0;
                if mated || self.config.mode == TacticsMode::BestMove {
                    if !mated && !line.is_empty() {
                        outs.push(Out::AddLine(line));
                    }
                    outs.extend(self.finish_solved());
                    return outs;
                }
                if let Some(reply) = reply {
                    outs.push(Out::Play(reply));
                    if let Ok(next) = active.current.after(reply) {
                        active.current = next;
                    }
                }
                active.remaining = Some(remaining);
                active.line = line;
                self.feedback = Feedback::Correct;
            }
            Verdict::Wrong(why) => {
                if let Some(asked) = self.try_opinion(mv, why) {
                    return asked;
                }
                outs.extend(self.register_wrong(why));
            }
            Verdict::Inconclusive => self.notice = Some(TacticsNotice::Inconclusive),
        }
        outs
    }

    /// A wrong move: an attempt is spent (and the problem lost with the
    /// last one).
    fn register_wrong(&mut self, why: Wrong) -> Vec<Out> {
        let Some(active) = self.active.as_mut() else {
            return Vec::new();
        };
        active.attempts_used = active.attempts_used.saturating_add(1);
        active.mistakes = active.mistakes.saturating_add(1);
        self.feedback = Feedback::Wrong(why);
        if active.attempts_used >= self.config.max_attempts {
            return self.fail(FailReason::NoAttempts);
        }
        Vec::new()
    }

    /// In "Best move" with the option on, a move that is only "not the
    /// move of the game" goes to the engine before it counts as wrong
    /// (D2130, D2131).
    fn try_opinion(&mut self, mv: Move, why: Wrong) -> Option<Vec<Out>> {
        if self.config.mode != TacticsMode::BestMove
            || !self.config.second_opinion
            || why != Wrong::NotBest
        {
            return None;
        }
        let engine = self.opinion_engine.clone()?;
        let active = self.active.as_ref()?;
        let best = *active.line.first()?;
        if best == mv {
            return None;
        }
        let cancel = Cancel::new();
        let job = OpinionJob {
            engine,
            position: active.current.clone(),
            user: mv,
            best,
            cancel: cancel.clone(),
        };
        self.opinion_cancel = Some(cancel);
        let request = self.request(PendingKind::Opinion);
        self.judging = Some(mv);
        self.set_phase(TacticsPhase::Judging);
        Some(vec![Out::Effect(TacticsEffect::Opinion {
            request,
            job: Box::new(job),
        })])
    }

    /// The answer to [`TacticsEffect::Opinion`] (D2131).
    pub fn opinion(&mut self, request: RequestId, verdict: OpinionVerdict) -> Vec<Out> {
        if !self.takes_answer(request, PendingKind::Opinion) {
            return Vec::new();
        }
        self.pending = None;
        self.opinion_cancel = None;
        let Some(mv) = self.judging.take() else {
            self.set_phase(TacticsPhase::Solving);
            return Vec::new();
        };
        self.set_phase(TacticsPhase::Solving);
        match verdict {
            // As good as the move of the game: the problem is solved, with
            // nothing to add to the line (D2132).
            OpinionVerdict::Equivalent => self.verdict(
                mv,
                Verdict::Correct {
                    mated: false,
                    reply: None,
                    remaining: 0,
                    line: Vec::new(),
                },
            ),
            OpinionVerdict::Worse => self.register_wrong(Wrong::NotBest),
            _ => {
                let outs = self.register_wrong(Wrong::NotBest);
                // After the change of phase of a lost problem the remark
                // would be wiped; it is said now (D2133).
                if self.phase == TacticsPhase::Solving {
                    self.notice = Some(TacticsNotice::OpinionUnavailable);
                }
                outs
            }
        }
    }

    /// "Continue against the engine" (D2140): the session ends without
    /// giving the game of the user back; the glue starts the game.
    pub fn release(&mut self) -> Vec<Out> {
        if !matches!(
            self.phase,
            TacticsPhase::Solved(_) | TacticsPhase::Failed(_)
        ) {
            return Vec::new();
        }
        self.dialog_open = false;
        self.pending = None;
        self.active = None;
        self.source = None;
        self.scan_left = 0;
        self.judging = None;
        self.cancel_find();
        self.cancel_opinion();
        self.feedback = Feedback::None;
        self.set_phase(TacticsPhase::Closed);
        let mut outs = vec![Out::Released];
        if self.used {
            outs.push(Out::SaveStore);
        }
        outs
    }

    fn finish_solved(&mut self) -> Vec<Out> {
        let Some(active) = self.active.as_ref() else {
            return Vec::new();
        };
        let outcome = if active.helped {
            ProblemOutcome::WithHelp
        } else if active.mistakes > 0 {
            ProblemOutcome::WithMistakes
        } else {
            ProblemOutcome::Perfect
        };
        let (id, ms) = (active.problem.id, active.elapsed_ms);
        self.store
            .stats
            .of_mut(self.config.mode)
            .record(outcome, ms);
        if let Some(source) = &self.source {
            self.store.mark_solved(&source.key.clone(), id);
        }
        self.feedback = Feedback::Correct;
        self.set_phase(TacticsPhase::Solved(outcome));
        vec![Out::SaveStore]
    }

    fn fail(&mut self, reason: FailReason) -> Vec<Out> {
        let Some(active) = self.active.as_ref() else {
            return Vec::new();
        };
        let ms = active.elapsed_ms;
        let line = active.line.clone();
        self.store
            .stats
            .of_mut(self.config.mode)
            .record(ProblemOutcome::Failed(reason), ms);
        self.set_phase(TacticsPhase::Failed(reason));
        let mut outs = Vec::new();
        if !line.is_empty() {
            outs.push(Out::AddLine(line));
        }
        outs.push(Out::SaveStore);
        outs
    }

    /// The hint button (D1919).
    pub fn hint_step(&mut self) {
        if self.phase != TacticsPhase::Solving || !self.config.hints {
            return;
        }
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.line.is_empty() {
            self.notice = Some(TacticsNotice::NoHint);
            return;
        }
        active.hint_level = (active.hint_level + 1).min(2);
        active.helped = true;
    }

    /// "Show the solution" (D1920).
    pub fn show_solution(&mut self) -> Vec<Out> {
        if self.phase != TacticsPhase::Solving {
            return Vec::new();
        }
        self.fail(FailReason::GaveUp)
    }

    /// "Next problem" (D1922).
    pub fn next_problem(&mut self) -> Vec<Out> {
        match self.phase {
            TacticsPhase::Solving => {
                self.store.stats.of_mut(self.config.mode).record_skipped();
            }
            TacticsPhase::Solved(_) | TacticsPhase::Failed(_) => {}
            _ => return Vec::new(),
        }
        self.active = None;
        self.notice = None;
        let mut outs = self.request_next();
        outs.push(Out::SaveStore);
        outs
    }

    /// "Try again" after a failure (D1928).
    pub fn retry(&mut self) -> Vec<Out> {
        if !matches!(self.phase, TacticsPhase::Failed(_)) {
            return Vec::new();
        }
        let Some(active) = self.active.as_mut() else {
            return Vec::new();
        };
        active.current = active.problem.position.clone();
        active.remaining = active.initial_remaining;
        active.line = active.initial_line.clone();
        active.attempts_used = 0;
        active.mistakes = 0;
        active.helped = false;
        active.hint_level = 0;
        active.elapsed_ms = 0;
        let position = active.problem.position.clone();
        self.feedback = Feedback::None;
        self.set_phase(TacticsPhase::Solving);
        // The failure was already counted as an ended problem; the new try
        // is shown again, so shown never falls behind ended (D1951).
        self.store.stats.of_mut(self.config.mode).record_shown();
        self.used = true;
        vec![Out::ShowProblem(Box::new(position))]
    }

    // ---- time ----

    /// The one-second tick of the front-end (D1925, D1934).
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let delta = self
            .last_tick
            .map(|last| {
                now.saturating_duration_since(last)
                    .min(Duration::from_millis(MAX_TICK_MS))
            })
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(MAX_TICK_MS));
        self.last_tick = Some(now);
        if !self.phase.wants_ticks() {
            return Vec::new();
        }
        // The time of the engine is not the time of the user (D2166): it
        // counts neither for the score nor for the limit.
        let engine_thinking = self.pending.is_some_and(|p| p.kind == PendingKind::Opinion);
        if matches!(self.phase, TacticsPhase::Solving | TacticsPhase::Judging)
            && !engine_thinking
            && let Some(active) = self.active.as_mut()
        {
            active.elapsed_ms = active.elapsed_ms.saturating_add(delta);
        }
        if let Some(pending) = self.pending.as_mut() {
            pending.waited_ms = pending.waited_ms.saturating_add(delta);
            if pending.waited_ms >= WATCHDOG_MS {
                return self.abandon();
            }
        }
        let limit_ms = u64::from(self.config.time_limit_s) * 1000;
        if self.phase == TacticsPhase::Solving
            && limit_ms > 0
            && self
                .active
                .as_ref()
                .is_some_and(|a| a.elapsed_ms >= limit_ms)
        {
            return self.fail(FailReason::TimeUp);
        }
        Vec::new()
    }

    /// Drops the request that waited too long (D1934); the answer, if it
    /// comes, is discarded because its id no longer matches.
    fn abandon(&mut self) -> Vec<Out> {
        let kind = self.pending.take().map(|p| p.kind);
        self.cancel_find();
        self.cancel_opinion();
        let outs = match kind {
            Some(PendingKind::Judge | PendingKind::Opinion) => {
                self.judging = None;
                self.set_phase(TacticsPhase::Solving);
                Vec::new()
            }
            _ => self.end_session(true),
        };
        self.notice = Some(TacticsNotice::TimedOut);
        outs
    }
}
