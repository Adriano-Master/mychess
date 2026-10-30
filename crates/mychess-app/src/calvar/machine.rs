//! The state machine of the calculation exercise (spec M8-6, D2302-D2306,
//! D2310, D2319).
//!
//! Pure: [`Calvar`] never touches the editor, the clock, the engine or the
//! disk. Every method returns [`Out`] commands that the glue
//! ([`flow`](super::flow)) runs: effects, the board to show, saving the
//! store, putting the previous game back, adding variants to it.

use std::time::{Duration, Instant};

use mychess_core::{Color, Move, Position, Role, Square};

use super::compare::{Report, compare};
use super::config::{CalvarConfig, PositionSource};
use super::engine::{CalvarEngine, EngineFailure, LinesJob, RootJob};
use super::eval::{LineProbes, RootLine};
use super::line::{Assessment, MAX_ENTRY_CHARS, MAX_LINE_PLIES, MAX_LINES, UserLine, read_moves};
use super::message::{CalvarEffect, CalvarNotice};
use super::review::ReviewRecord;
use super::store::{CalvarStats, CalvarStore, HistoryEntry, LoadedStore};
use crate::error::{FileError, RequestId};
use crate::session::EngineLineToAdd;
use crate::tactics::Cancel;

/// A read of a game for the start waits this long (in ticks) before it is
/// given up (D2371).
pub const GAME_WATCHDOG_MS: u64 = 20_000;
/// Rows of the history offered for review (D2370).
pub const REVIEW_ROWS: usize = 10;

/// Where the position of the exercise came from (M8-6d, D2367).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalvarOrigin {
    /// The position at the cursor of the editor.
    Editor,
    /// A FEN typed in the dialog.
    Fen,
    /// A game of the list of a database (its number).
    Game(u32),
    /// The problem on the board of the tactics training.
    Tactic,
    /// A past exercise being reviewed: when it was done.
    Review(u64),
}

/// A game being read for the start (D2371).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameWait {
    /// The request.
    pub request: RequestId,
    /// The number of the game.
    pub number: u32,
    /// Its value in the filter (D2368).
    pub raw_filter: u8,
    /// The wall clock for the history.
    pub now: u64,
    waited_ms: u64,
}

/// One row of the history offered for review (D2370).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    /// The place in the history, oldest first.
    pub index: usize,
    /// When it ended, seconds since the Unix epoch.
    pub when: u64,
    /// The score.
    pub score: u8,
    /// Good moves found.
    pub found: u8,
    /// Good moves there were.
    pub good: u8,
    /// Lines given.
    pub lines: u8,
    /// The lines were kept, so it can be reviewed.
    pub reviewable: bool,
}

/// A request that waits this long past its budget (in ticks) is dropped
/// (D2310).
pub const WATCHDOG_MS: u64 = 60_000;
/// The most that one tick adds to a clock (D2306).
pub const MAX_TICK_MS: u64 = 2_000;

/// Why an exercise ended without a result (D2319).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalvarFailure {
    /// The engine did not start, died or gave nothing.
    EngineUnavailable,
    /// A request took too long and was dropped.
    TimedOut,
    /// The time limit ran out with no line to evaluate.
    TimeUp,
}

/// Where the exercise is (D2319).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalvarPhase {
    /// No exercise.
    Closed,
    /// The user is calculating; the engine thinks in the background.
    Entering,
    /// "Done" was pressed: the engine evaluates the lines.
    Evaluating,
    /// The result is on the screen, with this score.
    Reported(u8),
    /// The exercise ended without a result.
    Failed(CalvarFailure),
}

impl CalvarPhase {
    /// Whether an exercise is on.
    pub fn is_live(self) -> bool {
        self != CalvarPhase::Closed
    }

    /// Whether the front-end must send ticks.
    pub fn wants_ticks(self) -> bool {
        matches!(self, CalvarPhase::Entering | CalvarPhase::Evaluating)
    }
}

/// A command for the glue.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Out {
    /// Run an effect.
    Effect(CalvarEffect),
    /// Put this in the dedicated game of the editor: the start position
    /// followed by `moves` (empty when the board does not follow, D2303).
    ShowBoard {
        /// The start position of the exercise.
        root: Box<Position>,
        /// The moves to play on it.
        moves: Vec<Move>,
    },
    /// Write the store.
    SaveStore,
    /// The exercise ended: the previous game of the editor comes back.
    Closed,
    /// A start that came from outside the dialog could not go on (D2371):
    /// the glue shows the reason.
    StartFailed(CalvarNotice),
    /// Add these lines as variants at the position of the exercise, in the
    /// game that just came back (D2324).
    AddToGame {
        /// The start position, which the cursor must still be at.
        root: Box<Position>,
        /// The variants.
        lines: Vec<EngineLineToAdd>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Root,
    Lines,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    request: RequestId,
    kind: PendingKind,
    waited_ms: u64,
    limit_ms: u64,
}

/// The exercise on the board.
#[derive(Debug, Clone)]
struct Run {
    root: Position,
    source: PositionSource,
    config: CalvarConfig,
    /// `None` while reviewing: no engine is started (D2369).
    engine: Option<CalvarEngine>,
    origin: CalvarOrigin,
    lines: Vec<UserLine>,
    /// The finished lines in SAN, made once (D2359).
    lines_san: Vec<String>,
    current: Vec<Move>,
    /// The line in progress in SAN, made when it changes.
    current_san: String,
    /// Lines the user must give, made at the start.
    required: u8,
    entry: String,
    rejected: Option<String>,
    root_lines: Option<Vec<RootLine>>,
    elapsed_ms: u64,
    now: u64,
    report: Option<Report>,
    cancel: Cancel,
}

impl Run {
    /// The position after the line in progress: the mental board (D2302).
    fn scratch(&self) -> Position {
        let mut pos = self.root.clone();
        for mv in &self.current {
            match pos.after(*mv) {
                Ok(next) => pos = next,
                Err(_) => break,
            }
        }
        pos
    }
}

/// A line of the user as the panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct LineView {
    /// The line in SAN.
    pub san: String,
    /// The evaluation of the user.
    pub assessment: Assessment,
}

/// A read-only picture of the machine for the front-end.
#[derive(Debug, Clone, PartialEq)]
pub struct CalvarView {
    /// Where the exercise is.
    pub phase: CalvarPhase,
    /// Whether the configuration dialog is open.
    pub dialog_open: bool,
    /// The configuration of the last start (or of the file).
    pub config: CalvarConfig,
    /// The side that moves in the start position.
    pub mover: Option<Color>,
    /// The finished lines.
    pub lines: Vec<LineView>,
    /// The line in progress, in SAN.
    pub current_san: String,
    /// Moves in the line in progress.
    pub current_plies: usize,
    /// The text being typed at the board.
    pub entry: String,
    /// The text of the last move that was refused (D2334).
    pub rejected: Option<String>,
    /// Lines asked for.
    pub required: u8,
    /// "Done" works.
    pub can_done: bool,
    /// "Take back" works.
    pub can_undo: bool,
    /// The buttons of the evaluation work.
    pub can_end_line: bool,
    /// The engine finished the first stage.
    pub root_ready: bool,
    /// The name of the engine.
    pub engine_name: String,
    /// Seconds spent calculating.
    pub elapsed_s: u32,
    /// The time limit in seconds, 0 for none.
    pub time_limit_s: u16,
    /// A remark, if any.
    pub notice: Option<CalvarNotice>,
    /// Why the file is not being saved, for as long as that holds.
    pub store_issue: Option<CalvarNotice>,
    /// The counters.
    pub stats: CalvarStats,
    /// The score of the last exercises, newest last.
    pub recent_scores: Vec<u8>,
    /// The result.
    pub report: Option<Report>,
    /// The best missed move, for the arrow of the board (D2316).
    pub arrow: Option<(Square, Square)>,
    /// Where the position came from (D2367).
    pub origin: Option<CalvarOrigin>,
    /// The last exercises of the history, newest first (D2370).
    pub history: Vec<HistoryRow>,
    /// A game is being read for the start (D2371).
    pub reading_game: bool,
}

/// The machine.
#[derive(Debug)]
pub struct Calvar {
    phase: CalvarPhase,
    dialog_open: bool,
    config: CalvarConfig,
    store: CalvarStore,
    store_loaded: bool,
    store_loading: bool,
    store_issue: Option<CalvarNotice>,
    used: bool,
    /// The store has something the file does not (D2357).
    store_dirty: bool,
    run: Option<Run>,
    next_request: u64,
    pending: Option<Pending>,
    notice: Option<CalvarNotice>,
    last_tick: Option<Instant>,
    game_wait: Option<GameWait>,
}

impl Default for Calvar {
    fn default() -> Calvar {
        Calvar {
            phase: CalvarPhase::Closed,
            dialog_open: false,
            config: CalvarConfig::default(),
            store: CalvarStore::default(),
            store_loaded: false,
            store_loading: false,
            store_issue: None,
            used: false,
            store_dirty: false,
            run: None,
            next_request: 0,
            pending: None,
            notice: None,
            last_tick: None,
            game_wait: None,
        }
    }
}

impl Calvar {
    /// A machine with nothing loaded.
    pub fn new() -> Calvar {
        Calvar::default()
    }

    // ---- reading ----

    /// Where the exercise is.
    pub fn phase(&self) -> CalvarPhase {
        self.phase
    }

    /// Whether an exercise is on.
    pub fn is_live(&self) -> bool {
        self.phase.is_live()
    }

    /// Whether the dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.dialog_open
    }

    /// The configuration of the last start (or of the file).
    pub fn config(&self) -> &CalvarConfig {
        &self.config
    }

    /// The remark waiting to be shown.
    pub fn notice(&self) -> Option<CalvarNotice> {
        self.notice
    }

    /// Sets the remark.
    pub(crate) fn set_notice(&mut self, notice: CalvarNotice) {
        self.notice = Some(notice);
    }

    /// The editor must not change: the exercise is not over (D2323).
    pub fn locks_editor(&self) -> bool {
        matches!(self.phase, CalvarPhase::Entering | CalvarPhase::Evaluating)
    }

    /// The moves of the user are taken now.
    pub fn takes_moves(&self) -> bool {
        self.phase == CalvarPhase::Entering
    }

    /// Whether the board follows the line in progress (D2303).
    pub fn follows_board(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.config.follow_board)
    }

    /// Whether the result could be added to the game of the user: it is on
    /// the screen and the position came from the editor (D2324). The glue
    /// adds the condition about the game tree.
    pub fn can_add_to_game(&self) -> bool {
        matches!(self.phase, CalvarPhase::Reported(_))
            && self
                .run
                .as_ref()
                .is_some_and(|r| r.source == PositionSource::Current && r.report.is_some())
    }

    /// The start position of the exercise.
    pub fn root(&self) -> Option<&Position> {
        self.run.as_ref().map(|r| &r.root)
    }

    /// Where the position of the exercise came from.
    pub fn source(&self) -> Option<PositionSource> {
        self.run.as_ref().map(|r| r.source)
    }

    /// The data to write.
    pub fn store(&self) -> &CalvarStore {
        &self.store
    }

    /// Whether `calvar.toml` must be left alone (D2317).
    pub fn store_read_only(&self) -> bool {
        self.store_issue.is_some()
    }

    /// A picture for the front-end.
    pub fn view(&self) -> CalvarView {
        let run = self.run.as_ref();
        let required = run.map_or(self.config.lines_required, |r| r.required);
        let live_entering = self.phase == CalvarPhase::Entering;
        let ready = run.is_some_and(|r| {
            r.current.is_empty() && r.lines.len() >= usize::from(required) && live_entering
        });
        CalvarView {
            phase: self.phase,
            dialog_open: self.dialog_open,
            config: run.map_or_else(|| self.config.clone(), |r| r.config.clone()),
            mover: run.map(|r| r.root.turn()),
            lines: run.map_or_else(Vec::new, |r| {
                r.lines
                    .iter()
                    .zip(&r.lines_san)
                    .map(|(l, san)| LineView {
                        san: san.clone(),
                        assessment: l.assessment,
                    })
                    .collect()
            }),
            current_san: run.map_or_else(String::new, |r| r.current_san.clone()),
            current_plies: run.map_or(0, |r| r.current.len()),
            entry: run.map_or_else(String::new, |r| r.entry.clone()),
            rejected: run.and_then(|r| r.rejected.clone()),
            required,
            can_done: ready,
            can_undo: live_entering && run.is_some_and(|r| !r.current.is_empty()),
            can_end_line: live_entering && run.is_some_and(|r| !r.current.is_empty()),
            root_ready: run.is_some_and(|r| r.root_lines.is_some()),
            engine_name: run
                .and_then(|r| r.engine.as_ref())
                .map_or_else(String::new, |e| e.0.name.clone()),
            elapsed_s: run.map_or(0, |r| {
                u32::try_from(r.elapsed_ms / 1000).unwrap_or(u32::MAX)
            }),
            time_limit_s: run.map_or(self.config.time_limit_s, |r| r.config.time_limit_s),
            notice: self.notice,
            store_issue: self.store_issue,
            stats: self.store.stats,
            recent_scores: self
                .store
                .history
                .iter()
                .rev()
                .take(10)
                .rev()
                .map(|e| e.score)
                .collect(),
            report: run.and_then(|r| r.report.clone()),
            arrow: run
                .and_then(|r| r.report.as_ref())
                .and_then(Report::best_missed)
                .and_then(|g| Some((g.mv.from()?, g.mv.to()?))),
            origin: run.map(|r| r.origin),
            history: self.history_rows(),
            reading_game: self.game_wait.is_some(),
        }
    }

    /// Lines the user must give: what was asked, but no more than the
    /// position has moves (D2305).
    fn required_lines(asked: u8, root: &Position) -> u8 {
        let moves = u8::try_from(root.legal_moves().len()).unwrap_or(u8::MAX);
        asked.min(moves).max(1)
    }

    // ---- the store ----

    /// Whether the read was asked for.
    pub fn store_requested(&self) -> bool {
        self.store_loaded || self.store_loading
    }

    /// Whether the file was read (or there was nothing to read yet is not
    /// enough: the answer must have arrived), D2354.
    pub fn store_ready(&self) -> bool {
        self.store_loaded
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
                CalvarNotice::StoreNewer
            } else {
                CalvarNotice::StoreUnreadable
            });
        } else if loaded.unreadable {
            self.notice = Some(CalvarNotice::StoreUnreadable);
        }
        // What this run already recorded is newer than the file.
        if !self.used {
            self.store = loaded.store;
        }
        if !self.is_live() {
            self.config = self.store.config.clone();
        }
    }

    /// The write finished.
    pub fn store_saved(&mut self, result: &Result<(), FileError>) {
        if result.is_err() {
            self.notice = Some(CalvarNotice::StoreNotSaved);
        }
    }

    /// Clears the counters and the history (D2318).
    pub fn reset_stats(&mut self) -> Vec<Out> {
        self.store.reset();
        self.used = true;
        self.store_dirty = false;
        vec![Out::SaveStore]
    }

    // ---- the history and the review (M8-6d) ----

    /// The last exercises of the history, newest first (D2370).
    fn history_rows(&self) -> Vec<HistoryRow> {
        self.store
            .history
            .iter()
            .enumerate()
            .rev()
            .take(REVIEW_ROWS)
            .map(|(index, e)| HistoryRow {
                index,
                when: e.when,
                score: e.score,
                found: e.found,
                good: e.good,
                lines: e.lines,
                reviewable: e.detail.is_some(),
            })
            .collect()
    }

    /// Opens entry `index` of the history in read-only mode (D2369): the
    /// result is rebuilt from what was saved, no engine is started and
    /// nothing is recorded. The glue has put the editor in order before.
    /// A refusal sets the notice and changes nothing.
    pub fn review(&mut self, index: usize) -> Vec<Out> {
        if self.is_live() {
            return Vec::new();
        }
        let Some(entry) = self.store.history.get(index) else {
            self.notice = Some(CalvarNotice::NoReview);
            return Vec::new();
        };
        let (Some(detail), Ok(root)) = (entry.detail.as_deref(), Position::from_fen(&entry.fen))
        else {
            self.notice = Some(CalvarNotice::NoReview);
            return Vec::new();
        };
        let Some(report) = detail.to_report(&root, entry.score) else {
            self.notice = Some(CalvarNotice::NoReview);
            return Vec::new();
        };
        let (score, when) = (entry.score, entry.when);
        self.dialog_open = false;
        self.notice = None;
        self.last_tick = None;
        self.pending = None;
        let config = self.config.clone();
        self.run = Some(Run {
            root: root.clone(),
            source: PositionSource::Fen,
            required: 0,
            config,
            engine: None,
            origin: CalvarOrigin::Review(when),
            lines: Vec::new(),
            lines_san: Vec::new(),
            current: Vec::new(),
            current_san: String::new(),
            entry: String::new(),
            rejected: None,
            root_lines: None,
            elapsed_ms: 0,
            now: when,
            report: Some(report),
            cancel: Cancel::new(),
        });
        self.phase = CalvarPhase::Reported(score);
        vec![Out::ShowBoard {
            root: Box::new(root),
            moves: Vec::new(),
        }]
    }

    /// Whether the exercise on the screen is a review.
    pub fn is_review(&self) -> bool {
        matches!(
            self.run.as_ref().map(|r| r.origin),
            Some(CalvarOrigin::Review(_))
        )
    }

    /// A game is going to be read for a start; returns the request that
    /// the answer must carry. An older read is given up (D2371).
    pub fn expect_game(&mut self, number: u32, raw_filter: u8, now: u64) -> RequestId {
        self.next_request = self.next_request.wrapping_add(1);
        let request = RequestId(self.next_request);
        self.game_wait = Some(GameWait {
            request,
            number,
            raw_filter,
            now,
            waited_ms: 0,
        });
        request
    }

    /// The read that waits, if `request` is its answer; an old answer gives
    /// `None` and the wait stays.
    pub fn take_game_wait(&mut self, request: RequestId) -> Option<GameWait> {
        if self.game_wait.is_some_and(|w| w.request == request) {
            return self.game_wait.take();
        }
        None
    }

    /// Gives up the read that waits (the dialog closed, the program quits).
    pub fn forget_game_wait(&mut self) {
        self.game_wait = None;
    }

    /// Whether a game is being read for a start.
    pub fn waiting_for_game(&self) -> bool {
        self.game_wait.is_some()
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

    // ---- starting and ending ----

    fn request(&mut self, kind: PendingKind, budget: Duration) -> RequestId {
        self.next_request = self.next_request.wrapping_add(1);
        let request = RequestId(self.next_request);
        let budget_ms = u64::try_from(budget.as_millis()).unwrap_or(u64::MAX);
        self.pending = Some(Pending {
            request,
            kind,
            waited_ms: 0,
            limit_ms: budget_ms.saturating_add(WATCHDOG_MS),
        });
        request
    }

    /// Starts an exercise at `root` (D2319). A position without moves is
    /// refused and nothing changes.
    pub fn start(
        &mut self,
        config: CalvarConfig,
        root: Position,
        engine: CalvarEngine,
        now: u64,
    ) -> Vec<Out> {
        let origin = match config.source {
            PositionSource::Current => CalvarOrigin::Editor,
            PositionSource::Fen => CalvarOrigin::Fen,
        };
        self.start_from(config, root, engine, now, origin)
    }

    /// Like [`Calvar::start`] for a position that does not come from the
    /// dialog (a game of the list, a tactics problem): `origin` says where
    /// it came from (D2367). Such a position is never the one of the editor,
    /// so the result cannot be added to the game of the user.
    pub fn start_from(
        &mut self,
        config: CalvarConfig,
        root: Position,
        engine: CalvarEngine,
        now: u64,
        origin: CalvarOrigin,
    ) -> Vec<Out> {
        if self.is_live() {
            return Vec::new();
        }
        if root.legal_moves().is_empty() {
            self.notice = Some(CalvarNotice::NoMoves);
            return Vec::new();
        }
        let config = config.sanitized();
        self.config = config.clone();
        if self.store.config != config {
            self.store.config = config.clone();
            self.store_dirty = true;
        }
        self.used = true;
        self.dialog_open = false;
        self.notice = None;
        self.last_tick = None;
        let cancel = Cancel::new();
        let job = RootJob {
            engine: engine.clone(),
            position: root.clone(),
            multipv: config.multipv,
            think: Duration::from_millis(config.root_ms),
            cancel: cancel.clone(),
        };
        let budget = job.budget();
        let request = self.request(PendingKind::Root, budget);
        self.run = Some(Run {
            root: root.clone(),
            source: match origin {
                CalvarOrigin::Editor => PositionSource::Current,
                _ => PositionSource::Fen,
            },
            origin,
            required: Self::required_lines(config.lines_required, &root),
            config,
            engine: Some(engine),
            lines: Vec::new(),
            lines_san: Vec::new(),
            current: Vec::new(),
            current_san: String::new(),
            entry: String::new(),
            rejected: None,
            root_lines: None,
            elapsed_ms: 0,
            now,
            report: None,
            cancel,
        });
        self.phase = CalvarPhase::Entering;
        vec![
            Out::ShowBoard {
                root: Box::new(root),
                moves: Vec::new(),
            },
            Out::Effect(CalvarEffect::AnalyzeRoot {
                request,
                job: Box::new(job),
            }),
        ]
    }

    fn end_session(&mut self) -> Vec<Out> {
        if let Some(run) = self.run.take() {
            run.cancel.cancel();
        }
        self.pending = None;
        self.dialog_open = false;
        self.phase = CalvarPhase::Closed;
        let mut outs = vec![Out::Closed];
        if std::mem::take(&mut self.store_dirty) {
            outs.push(Out::SaveStore);
        }
        outs
    }

    /// Ends the exercise and puts the previous game back (D2320).
    pub fn close(&mut self) -> Vec<Out> {
        self.dialog_open = false;
        if !self.is_live() {
            return Vec::new();
        }
        self.end_session()
    }

    /// Ends the exercise and adds its lines to the game as variants (D2324).
    pub fn add_to_game(&mut self) -> Vec<Out> {
        if !self.can_add_to_game() {
            return Vec::new();
        }
        let Some(run) = self.run.as_ref() else {
            return Vec::new();
        };
        let lines = Self::variants(run);
        let root = Box::new(run.root.clone());
        let mut outs = self.end_session();
        outs.push(Out::AddToGame { root, lines });
        outs
    }

    /// The variants of D2324 as they stand now (empty before the result).
    pub(crate) fn variants_now(&self) -> Vec<EngineLineToAdd> {
        self.run.as_ref().map(Self::variants).unwrap_or_default()
    }

    /// The variants of D2324: the lines of the user and the good moves that
    /// were missed, with comments that read the same in every language.
    fn variants(run: &Run) -> Vec<EngineLineToAdd> {
        let Some(report) = run.report.as_ref() else {
            return Vec::new();
        };
        let mut lines: Vec<EngineLineToAdd> = report
            .lines
            .iter()
            .map(|l| {
                let comment = match l.end.or(l.first) {
                    Some(eval) => format!("{} {}", l.assessment.symbol(), eval.label()),
                    None => l.assessment.symbol().to_owned(),
                };
                EngineLineToAdd {
                    pv: l.moves.clone(),
                    comment: Some(comment),
                }
            })
            .collect();
        lines.extend(
            report
                .good
                .iter()
                .filter(|g| !g.found)
                .map(|g| EngineLineToAdd {
                    pv: g.pv.clone(),
                    comment: Some(format!("d{}:{}", g.depth, g.eval.label())),
                }),
        );
        lines
    }

    // ---- entering the lines ----

    fn board_out(run: &mut Run) -> Out {
        run.current_san = super::line::san_line(&run.root, &run.current);
        Out::ShowBoard {
            root: Box::new(run.root.clone()),
            moves: if run.config.follow_board {
                run.current.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// Whether a user action may change the lines now.
    fn editing(&mut self) -> bool {
        if self.phase == CalvarPhase::Entering && self.run.is_some() {
            self.notice = None;
            return true;
        }
        false
    }

    /// Moves typed by the user (D2302, D2334).
    pub fn enter_text(&mut self, text: &str) -> Vec<Out> {
        if !self.editing() {
            return Vec::new();
        }
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        run.entry.clear();
        run.rejected = None;
        let read = read_moves(&run.scratch(), text);
        let room = MAX_LINE_PLIES.saturating_sub(run.current.len());
        let too_long = read.moves.len() > room;
        let before = run.current.len();
        run.current.extend(read.moves.into_iter().take(room));
        let changed = run.current.len() != before;
        if too_long {
            self.notice = Some(CalvarNotice::LineTooLong);
        } else if read.rejected.is_some() {
            run.rejected = read.rejected;
            self.notice = Some(CalvarNotice::IllegalMove);
        }
        if changed {
            return vec![Self::board_out(run)];
        }
        Vec::new()
    }

    /// A move made on the board (D2303): it counts when it is legal in the
    /// line being calculated, with the same squares and promotion.
    pub fn board_move(&mut self, mv: Move) -> Vec<Out> {
        let (from, to) = (mv.from(), mv.to());
        self.board_squares(from, to, mv.promotion())
    }

    /// Like [`Calvar::board_move`], from the squares and the promotion piece.
    pub fn board_squares(
        &mut self,
        from: Option<Square>,
        to: Option<Square>,
        promotion: Option<Role>,
    ) -> Vec<Out> {
        if !self.editing() {
            return Vec::new();
        }
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        run.rejected = None;
        if run.current.len() >= MAX_LINE_PLIES {
            self.notice = Some(CalvarNotice::LineTooLong);
            return Vec::new();
        }
        let scratch = run.scratch();
        let legal = scratch.legal_moves();
        let same_squares = |m: &&Move| from.is_some() && m.from() == from && m.to() == to;
        // A promotion without a piece is a queen (as in the move list).
        let found = legal
            .iter()
            .filter(same_squares)
            .find(|m| m.promotion() == promotion)
            .or_else(|| {
                legal
                    .iter()
                    .filter(same_squares)
                    .find(|m| promotion.is_none() && m.promotion() == Some(Role::Queen))
            })
            .copied();
        let Some(m) = found else {
            self.notice = Some(CalvarNotice::UseKeyboard);
            return Vec::new();
        };
        run.current.push(m);
        vec![Self::board_out(run)]
    }

    /// A character typed at the board.
    pub fn entry_char(&mut self, c: char) {
        if !self.editing() {
            return;
        }
        if let Some(run) = self.run.as_mut()
            && run.entry.chars().count() < MAX_ENTRY_CHARS
        {
            run.entry.push(c);
        }
    }

    /// Backspace at the board.
    pub fn entry_backspace(&mut self) {
        if let Some(run) = self.run.as_mut() {
            run.entry.pop();
        }
    }

    /// Escape at the board.
    pub fn entry_clear(&mut self) {
        if let Some(run) = self.run.as_mut() {
            run.entry.clear();
        }
    }

    /// Enter at the board: the text typed becomes moves.
    pub fn entry_commit(&mut self) -> Vec<Out> {
        let text = self
            .run
            .as_ref()
            .map(|r| r.entry.clone())
            .unwrap_or_default();
        self.enter_text(&text)
    }

    /// Takes back the last move of the line in progress.
    pub fn undo(&mut self) -> Vec<Out> {
        if !self.editing() {
            return Vec::new();
        }
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        run.rejected = None;
        if run.current.pop().is_none() {
            self.notice = Some(CalvarNotice::EmptyLine);
            return Vec::new();
        }
        vec![Self::board_out(run)]
    }

    /// Ends the line in progress with the evaluation of the user (D2304).
    pub fn end_line(&mut self, assessment: Assessment) -> Vec<Out> {
        if !self.editing() {
            return Vec::new();
        }
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        run.rejected = None;
        if run.current.is_empty() {
            self.notice = Some(CalvarNotice::EmptyLine);
            return Vec::new();
        }
        if run.lines.len() >= MAX_LINES {
            self.notice = Some(CalvarNotice::TooManyLines);
            return Vec::new();
        }
        let moves = std::mem::take(&mut run.current);
        run.lines_san.push(super::line::san_line(&run.root, &moves));
        run.lines.push(UserLine { moves, assessment });
        vec![Self::board_out(run)]
    }

    /// Removes a finished line.
    pub fn remove_line(&mut self, index: usize) -> Vec<Out> {
        if !self.editing() {
            return Vec::new();
        }
        if let Some(run) = self.run.as_mut()
            && index < run.lines.len()
        {
            run.lines.remove(index);
            run.lines_san.remove(index);
        }
        Vec::new()
    }

    // ---- evaluating ----

    /// "Done" (D2305).
    pub fn done(&mut self) -> Vec<Out> {
        if !self.editing() {
            return Vec::new();
        }
        let Some(run) = self.run.as_ref() else {
            return Vec::new();
        };
        if !run.current.is_empty() {
            self.notice = Some(CalvarNotice::LineOpen);
            return Vec::new();
        }
        if run.lines.len() < usize::from(run.required) {
            self.notice = Some(CalvarNotice::NeedMoreLines);
            return Vec::new();
        }
        self.begin_evaluating()
    }

    fn begin_evaluating(&mut self) -> Vec<Out> {
        self.phase = CalvarPhase::Evaluating;
        self.lines_job()
    }

    /// Sends the second stage when the first is in.
    fn lines_job(&mut self) -> Vec<Out> {
        let Some(run) = self.run.as_ref() else {
            return Vec::new();
        };
        if run.root_lines.is_none() || self.pending.is_some() {
            return Vec::new();
        }
        let Some(engine) = run.engine.clone() else {
            return Vec::new();
        };
        let job = LinesJob {
            engine,
            start: run.root.clone(),
            lines: run.lines.iter().map(|l| l.moves.clone()).collect(),
            per_search: Duration::from_millis(run.config.line_ms),
            cancel: run.cancel.clone(),
        };
        let budget = job.budget();
        let request = self.request(PendingKind::Lines, budget);
        vec![Out::Effect(CalvarEffect::AnalyzeLines {
            request,
            job: Box::new(job),
        })]
    }

    fn fail(&mut self, failure: CalvarFailure, notice: CalvarNotice) -> Vec<Out> {
        if let Some(run) = self.run.as_ref() {
            run.cancel.cancel();
        }
        self.pending = None;
        self.phase = CalvarPhase::Failed(failure);
        self.notice = Some(notice);
        Vec::new()
    }

    fn pending_is(&mut self, request: RequestId, kind: PendingKind) -> bool {
        let matches = self
            .pending
            .is_some_and(|p| p.request == request && p.kind == kind);
        if matches {
            self.pending = None;
        }
        matches && self.run.is_some()
    }

    /// The first stage answered (D2308, D2310).
    pub fn root_analyzed(
        &mut self,
        request: RequestId,
        result: Result<Vec<RootLine>, EngineFailure>,
    ) -> Vec<Out> {
        if !self.pending_is(request, PendingKind::Root) || !self.phase.is_live() {
            return Vec::new();
        }
        match result {
            Ok(lines) if !lines.is_empty() => {
                if let Some(run) = self.run.as_mut() {
                    run.root_lines = Some(lines);
                }
                if self.phase == CalvarPhase::Evaluating {
                    return self.lines_job();
                }
                Vec::new()
            }
            _ => self.fail(CalvarFailure::EngineUnavailable, CalvarNotice::EngineFailed),
        }
    }

    /// The second stage answered (D2308, D2310).
    pub fn lines_analyzed(
        &mut self,
        request: RequestId,
        result: Result<Vec<LineProbes>, EngineFailure>,
    ) -> Vec<Out> {
        if !self.pending_is(request, PendingKind::Lines) || self.phase != CalvarPhase::Evaluating {
            return Vec::new();
        }
        // An answer in which no line has a single usable probe is a failed
        // engine, not a result of zeros (D2355): it is not put in the
        // history.
        let probes = match result {
            Ok(probes) if probes.iter().any(|p| p.first.is_some() || p.end.is_some()) => probes,
            _ => {
                return self.fail(CalvarFailure::EngineUnavailable, CalvarNotice::EngineFailed);
            }
        };
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        let root_lines = run.root_lines.clone().unwrap_or_default();
        let report = compare(&run.root, &root_lines, &run.lines, &probes);
        let entry = HistoryEntry {
            when: run.now,
            score: report.score,
            found: report.found,
            good: report.good_total,
            lines: u8::try_from(run.lines.len()).unwrap_or(u8::MAX),
            crucial: report.has_crucial_miss(),
            white_to_move: run.root.turn() == Color::White,
            fen: run.root.fen(),
            detail: Some(Box::new(ReviewRecord::from_report(&run.root, &report))),
        };
        let score = report.score;
        run.report = Some(report);
        run.cancel.cancel();
        self.store.record(entry);
        self.used = true;
        self.store_dirty = false;
        self.phase = CalvarPhase::Reported(score);
        self.notice = None;
        vec![Out::SaveStore]
    }

    // ---- time ----

    /// The tick of the front-end: the clock of the exercise and the
    /// watchdog (D2306, D2310).
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let delta = self
            .last_tick
            .map(|last| {
                now.saturating_duration_since(last)
                    .min(Duration::from_millis(MAX_TICK_MS))
            })
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(MAX_TICK_MS));
        self.last_tick = Some(now);
        if let Some(wait) = self.game_wait.as_mut() {
            wait.waited_ms = wait.waited_ms.saturating_add(delta);
            if wait.waited_ms >= GAME_WATCHDOG_MS {
                self.game_wait = None;
                return vec![Out::StartFailed(CalvarNotice::TimedOut)];
            }
        }
        if !self.phase.wants_ticks() {
            return Vec::new();
        }
        if let Some(pending) = self.pending.as_mut() {
            pending.waited_ms = pending.waited_ms.saturating_add(delta);
            if pending.waited_ms >= pending.limit_ms {
                return self.fail(CalvarFailure::TimedOut, CalvarNotice::TimedOut);
            }
        }
        if self.phase != CalvarPhase::Entering {
            return Vec::new();
        }
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        run.elapsed_ms = run.elapsed_ms.saturating_add(delta);
        let limit_ms = u64::from(run.config.time_limit_s) * 1000;
        if limit_ms > 0 && run.elapsed_ms >= limit_ms {
            return self.time_up();
        }
        Vec::new()
    }

    /// The time limit ran out (D2306): the lines finished so far are
    /// evaluated; the one in progress is dropped.
    fn time_up(&mut self) -> Vec<Out> {
        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };
        run.current.clear();
        if run.lines.is_empty() {
            return self.fail(CalvarFailure::TimeUp, CalvarNotice::TimeUp);
        }
        let mut outs = vec![Self::board_out(run)];
        outs.extend(self.begin_evaluating());
        outs
    }
}
