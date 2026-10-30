//! The state machine of the opening trainer: pure, without `AppState`, with
//! the time injected (spec M8-5, D2213-D2231).
//!
//! The machine keeps its own [`Position`] for the line in progress; the glue
//! shows the game in the editor from the [`Out`] commands. The judgement of a
//! move is a lookup in the repertoire, so it happens inside `update`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use mychess_core::{Color, Move, Position};

use super::build::{BuildError, BuildJob, BuildOutput, BuildSource};
use super::config::{OpeningConfig, TrainSide};
use super::message::{OpeningEffect, OpeningNotice};
use super::repertoire::{
    MAX_CANDIDATES, RepSet, Rng, WALK_SAMPLES, WALK_SAMPLES_NO_REPEAT, Walk, has_user_move,
    pick_weighted, sample_walk,
};
use super::stats::{PosStat, ReportView, SessionStats, need_of};
use super::store::{LoadedStore, OpeningStore};
use crate::error::{FileError, RequestId};
use crate::tactics::{Cancel, Hint};

/// A request that waits this long (in ticks) is dropped (D2231).
pub const WATCHDOG_MS: u64 = 60_000;
/// The most that one tick adds to a clock.
pub const MAX_TICK_MS: u64 = 2_000;

/// Why a line ended (D2219).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineEnd {
    /// The repertoire has no more moves.
    Complete,
    /// The maximum depth was reached.
    Depth,
}

/// Where the trainer is (D2228).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpeningPhase {
    /// No session.
    Closed,
    /// Reading the repertoire.
    Building,
    /// The user is to move.
    UserTurn,
    /// The line ended.
    LineDone(LineEnd),
    /// With "do not repeat", every line that could be drawn was played.
    Exhausted,
}

impl OpeningPhase {
    /// Whether a session is on.
    pub fn is_live(self) -> bool {
        self != OpeningPhase::Closed
    }

    /// Whether the front-end must send ticks (only the build waits).
    pub fn wants_ticks(self) -> bool {
        self == OpeningPhase::Building
    }
}

/// A command for the glue.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Out {
    /// Run an effect.
    Effect(OpeningEffect),
    /// Put a new game in the editor, parking the one of the user the first
    /// time; the board is turned to `flipped`.
    ShowGame {
        /// The user plays Black.
        flipped: bool,
    },
    /// Play the move at the end of the game of the editor.
    Play(Move),
    /// Write the store.
    SaveStore,
    /// The session ended: the previous game of the editor comes back.
    Closed,
}

/// A move of the repertoire for the panel (D2217).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The move in SAN.
    pub san: String,
    /// Its share of the weight of the position, in percent.
    pub percent: u8,
    /// The move is dubious.
    pub dubious: bool,
}

/// A picture of the trainer for the front-end.
#[derive(Debug, Clone, PartialEq)]
pub struct OpeningView {
    /// The phase.
    pub phase: OpeningPhase,
    /// The configuration dialog is open.
    pub dialog_open: bool,
    /// The configuration (the one of the session, or the last one).
    pub config: OpeningConfig,
    /// The remark of the last action.
    pub notice: Option<OpeningNotice>,
    /// The colour of the user in the line in progress.
    pub color: Option<Color>,
    /// The number of the line (from 1).
    pub line: u32,
    /// The plies played in the line.
    pub ply: usize,
    /// The hint on the board.
    pub hint: Option<Hint>,
    /// The moves of the repertoire in the position, when they are shown.
    pub candidates: Vec<Candidate>,
    /// The counters of the session.
    pub session: SessionStats,
    /// Errors in the position that the user is on.
    pub visit_errors: u32,
    /// The repertoire was cut by a limit (D2210).
    pub truncated: bool,
    /// Why the store is not written, while true (D2225).
    pub store_issue: Option<OpeningNotice>,
    /// The report, when shown (D2224).
    pub report: Option<ReportView>,
    /// A "next line" is possible.
    pub can_next: bool,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    request: RequestId,
    waited_ms: u64,
}

/// The position of the user that is being answered (D2222).
#[derive(Debug, Clone, Copy, Default)]
struct Visit {
    errors: u32,
    helped: bool,
}

/// The state machine (D2228).
#[derive(Debug)]
pub struct OpeningTrainer {
    phase: OpeningPhase,
    dialog_open: bool,
    notice: Option<OpeningNotice>,
    config: OpeningConfig,
    store: OpeningStore,
    store_requested: bool,
    /// `opening_trainer.toml` was read (D2255): nothing is written before.
    store_loaded: bool,
    /// Resets asked for before the file was read: they apply to what it
    /// brings too (D2255).
    resets_before_load: Vec<String>,
    read_only: bool,
    store_issue: Option<OpeningNotice>,
    /// The store has something to write (the config or a score changed).
    used: bool,
    pending: Option<Pending>,
    next_request: u64,
    build_cancel: Option<Cancel>,
    set: RepSet,
    /// The keys of the score of the white and of the black repertoire.
    keys: [String; 2],
    truncated: bool,
    rng: Rng,
    today: u32,
    first_color: Color,
    // The line in progress.
    pos: Position,
    ply: usize,
    color: Color,
    target: Vec<Move>,
    target_id: u64,
    on_target: bool,
    line_no: u32,
    line_errors: u32,
    visit: Visit,
    hint_level: u8,
    session: SessionStats,
    done_lines: HashSet<u64>,
    report: Option<ReportView>,
    last_tick: Option<Instant>,
}

impl Default for OpeningTrainer {
    fn default() -> OpeningTrainer {
        OpeningTrainer::new()
    }
}

impl OpeningTrainer {
    /// A closed trainer.
    pub fn new() -> OpeningTrainer {
        OpeningTrainer {
            phase: OpeningPhase::Closed,
            dialog_open: false,
            notice: None,
            config: OpeningConfig::default(),
            store: OpeningStore::default(),
            store_requested: false,
            store_loaded: false,
            resets_before_load: Vec::new(),
            read_only: false,
            store_issue: None,
            used: false,
            pending: None,
            next_request: 0,
            build_cancel: None,
            set: RepSet::default(),
            keys: [String::new(), String::new()],
            truncated: false,
            rng: Rng::new(1),
            today: 0,
            first_color: Color::White,
            pos: Position::new(),
            ply: 0,
            color: Color::White,
            target: Vec::new(),
            target_id: 0,
            on_target: false,
            line_no: 0,
            line_errors: 0,
            visit: Visit::default(),
            hint_level: 0,
            session: SessionStats::default(),
            done_lines: HashSet::new(),
            report: None,
            last_tick: None,
        }
    }

    // ---- accessors ----

    /// The phase.
    pub fn phase(&self) -> OpeningPhase {
        self.phase
    }

    /// Whether a session is on.
    pub fn is_live(&self) -> bool {
        self.phase.is_live()
    }

    /// Whether the configuration dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.dialog_open
    }

    /// The configuration.
    pub fn config(&self) -> &OpeningConfig {
        &self.config
    }

    /// The remark of the last action.
    pub fn notice(&self) -> Option<OpeningNotice> {
        self.notice
    }

    pub(crate) fn set_notice(&mut self, notice: OpeningNotice) {
        self.notice = Some(notice);
    }

    /// Takes the remark.
    pub fn take_notice(&mut self) -> Option<OpeningNotice> {
        self.notice.take()
    }

    /// Whether the user's moves are judged now.
    pub fn takes_moves(&self) -> bool {
        self.phase == OpeningPhase::UserTurn
    }

    /// Whether the editor is locked (D2230).
    pub fn locks_editor(&self) -> bool {
        self.phase == OpeningPhase::UserTurn
    }

    /// The colour of the user in the line in progress.
    pub fn color(&self) -> Option<Color> {
        self.is_live().then_some(self.color)
    }

    /// The position of the line in progress.
    pub fn position(&self) -> &Position {
        &self.pos
    }

    /// The counters of the session.
    pub fn session(&self) -> SessionStats {
        self.session
    }

    /// The hint on the board.
    pub fn hint(&self) -> Option<Hint> {
        if self.phase != OpeningPhase::UserTurn || self.hint_level == 0 {
            return None;
        }
        let best = self.rep()?.best(&self.pos)?;
        Some(Hint {
            from: best.mv.from()?,
            to: (self.hint_level >= 2).then(|| best.mv.to()).flatten(),
        })
    }

    fn rep(&self) -> Option<std::sync::Arc<super::repertoire::Repertoire>> {
        self.set.for_color(self.color).cloned()
    }

    fn key_of(&self, color: Color) -> &str {
        match color {
            Color::White => &self.keys[0],
            Color::Black => &self.keys[1],
        }
    }

    /// A picture of the trainer.
    pub fn view(&self) -> OpeningView {
        let live = self.is_live();
        OpeningView {
            phase: self.phase,
            dialog_open: self.dialog_open,
            config: self.config.clone(),
            notice: self.notice,
            color: self.color(),
            line: self.line_no,
            ply: self.ply,
            hint: self.hint(),
            candidates: self.candidates(),
            session: self.session,
            visit_errors: self.visit.errors,
            truncated: live && self.truncated,
            store_issue: self.store_issue,
            report: self.report,
            can_next: matches!(
                self.phase,
                OpeningPhase::UserTurn | OpeningPhase::LineDone(_)
            ),
        }
    }

    fn candidates(&self) -> Vec<Candidate> {
        let shown = self.config.show_candidates || self.visit.errors > 0 || self.visit.helped;
        if self.phase != OpeningPhase::UserTurn || !shown {
            return Vec::new();
        }
        let Some(rep) = self.rep() else {
            return Vec::new();
        };
        let moves = rep.moves(&self.pos);
        let total: u64 = moves.iter().map(|m| u64::from(m.weight)).sum();
        let mut sorted: Vec<_> = moves.iter().collect();
        sorted.sort_by_key(|m| std::cmp::Reverse(m.weight));
        sorted
            .into_iter()
            .take(MAX_CANDIDATES)
            .filter_map(|m| {
                let san = self.pos.san(m.mv).ok()?;
                let percent = (u64::from(m.weight) * 100)
                    .checked_div(total)
                    .and_then(|p| u8::try_from(p).ok())
                    .unwrap_or(0);
                Some(Candidate {
                    san,
                    percent,
                    dubious: m.dubious,
                })
            })
            .collect()
    }

    // ---- the store ----

    /// Whether the store was asked for.
    pub fn store_requested(&self) -> bool {
        self.store_requested
    }

    /// Marks the store as asked for.
    pub fn begin_load(&mut self) {
        self.store_requested = true;
    }

    /// Whether the store was read already: before that, writing it would
    /// erase the score or a file of a newer version (D2255).
    pub fn store_ready(&self) -> bool {
        self.store_loaded
    }

    /// The store was read.
    pub fn store_loaded(&mut self, loaded: LoadedStore) {
        self.store_loaded = true;
        let unused = !self.used;
        self.store.merge_loaded(loaded.store, self.used);
        for key in std::mem::take(&mut self.resets_before_load) {
            self.store.reset(&key);
        }
        if unused {
            self.config = self.store.config.clone();
        }
        self.read_only = loaded.read_only;
        self.store_issue = if loaded.newer {
            Some(OpeningNotice::StoreNewer)
        } else if loaded.unreadable {
            Some(OpeningNotice::StoreUnreadable)
        } else {
            None
        };
    }

    /// What is saved.
    pub fn store(&self) -> &OpeningStore {
        &self.store
    }

    /// Whether the file must not be written (D2225).
    pub fn store_read_only(&self) -> bool {
        self.read_only
    }

    /// The store was written (or not).
    pub fn store_saved(&mut self, result: &Result<(), FileError>) {
        if result.is_err() {
            self.notice = Some(OpeningNotice::StoreNotSaved);
        }
    }

    /// Opens the configuration dialog.
    pub fn open_dialog(&mut self) {
        self.dialog_open = true;
        self.notice = None;
    }

    /// Closes the dialog.
    pub fn close_dialog(&mut self) {
        self.dialog_open = false;
    }

    /// Clears the score of the repertoires named by `keys` (D2226).
    pub fn reset(&mut self, keys: &[String]) -> Vec<Out> {
        for key in keys {
            self.store.reset(key);
            if !self.store_loaded {
                self.resets_before_load.push(key.clone());
            }
        }
        self.report = None;
        self.used = true;
        vec![Out::SaveStore]
    }

    // ---- starting ----

    fn request(&mut self) -> RequestId {
        self.next_request = self.next_request.wrapping_add(1);
        let request = RequestId(self.next_request);
        self.pending = Some(Pending {
            request,
            waited_ms: 0,
        });
        request
    }

    fn set_phase(&mut self, phase: OpeningPhase) {
        // A remark is about the phase it was made in; the ones that explain
        // why the session ended stay.
        if phase != OpeningPhase::Closed {
            self.notice = None;
        }
        self.phase = phase;
        self.last_tick = None;
    }

    /// Starts a session (D2228). `source` is what the glue resolved from the
    /// configuration, with the keys of the score of each colour, or the
    /// remark that explains why there is none.
    pub fn start(
        &mut self,
        config: OpeningConfig,
        seed: u64,
        today: u32,
        source: Result<(BuildSource, [String; 2]), OpeningNotice>,
    ) -> Vec<Out> {
        if self.is_live() {
            return Vec::new();
        }
        let config = config.sanitized();
        let (source, keys) = match source {
            Ok(ok) => ok,
            Err(notice) => {
                self.notice = Some(notice);
                return Vec::new();
            }
        };
        self.store.config = config.clone();
        self.config = config;
        self.keys = keys;
        self.rng = Rng::new(seed);
        self.first_color = if self.rng.next_u64() & 1 == 0 {
            Color::White
        } else {
            Color::Black
        };
        self.today = today;
        self.set = RepSet::default();
        self.truncated = false;
        self.session = SessionStats::default();
        self.done_lines.clear();
        self.line_no = 0;
        self.line_errors = 0;
        self.visit = Visit::default();
        self.hint_level = 0;
        self.report = None;
        self.dialog_open = false;
        self.notice = None;
        self.used = true;
        let cancel = Cancel::new();
        self.build_cancel = Some(cancel.clone());
        let job = BuildJob {
            source,
            side: self.config.side,
            max_depth: self.config.max_depth,
            only_flagged: self.config.only_flagged,
            only_marked: self.config.only_marked,
            cancel,
        };
        let request = self.request();
        self.set_phase(OpeningPhase::Building);
        vec![
            Out::Effect(OpeningEffect::Build {
                request,
                job: Box::new(job),
            }),
            Out::SaveStore,
        ]
    }

    fn takes_answer(&self, request: RequestId) -> bool {
        self.phase == OpeningPhase::Building && self.pending.is_some_and(|p| p.request == request)
    }

    /// The answer to [`OpeningEffect::Build`].
    pub fn built(
        &mut self,
        request: RequestId,
        result: Result<Box<BuildOutput>, BuildError>,
    ) -> Vec<Out> {
        if !self.takes_answer(request) {
            return Vec::new();
        }
        self.pending = None;
        let output = match result {
            Ok(output) => *output,
            Err(BuildError::Cancelled) => return self.end_session(false),
            Err(error) => {
                let notice = match error {
                    BuildError::Book => OpeningNotice::BookUnreadable,
                    BuildError::Mask => OpeningNotice::MaskUnreadable,
                    BuildError::Source(crate::tactics::SourceError::NeedsUpgrade) => {
                        OpeningNotice::SourceNeedsUpgrade
                    }
                    BuildError::Source(crate::tactics::SourceError::Corrupt) => {
                        OpeningNotice::SourceCorrupt
                    }
                    BuildError::Source(crate::tactics::SourceError::Unfinished) => {
                        OpeningNotice::SourceUnfinished
                    }
                    _ => OpeningNotice::SourceUnreadable,
                };
                let outs = self.end_session(true);
                self.notice = Some(notice);
                return outs;
            }
        };
        self.truncated = output.truncated;
        self.set = output.set;
        // A colour is kept only when a line with a move of the user can be
        // drawn from its repertoire (D2212).
        let depth = usize::from(self.config.max_depth);
        let mut playable = [false, false];
        for (i, color) in [Color::White, Color::Black].into_iter().enumerate() {
            let wanted = match self.config.side {
                TrainSide::White => color == Color::White,
                TrainSide::Black => color == Color::Black,
                TrainSide::Both => true,
            };
            if let Some(rep) = self.set.for_color(color).filter(|_| wanted) {
                let walk = sample_walk(rep, color, depth, true, &mut Rng::new(1), &|_| 0);
                playable[i] = has_user_move(&walk, color);
            }
        }
        if !playable[0] {
            self.set.white = None;
        }
        if !playable[1] {
            self.set.black = None;
        }
        if !playable[0] && !playable[1] {
            let outs = self.end_session(true);
            self.notice = Some(OpeningNotice::EmptyRepertoire);
            return outs;
        }
        for (i, color) in [Color::White, Color::Black].into_iter().enumerate() {
            let key = self.key_of(color).to_owned();
            if playable[i] && !key.is_empty() {
                self.store.use_rep(&key);
            }
        }
        let outs = self.begin_line(false);
        if self.config.side == TrainSide::Both && playable[0] != playable[1] {
            self.notice = Some(OpeningNotice::OneSideEmpty);
        } else if self.truncated && self.phase == OpeningPhase::UserTurn {
            self.notice = Some(OpeningNotice::Truncated);
        }
        outs
    }

    // ---- lines ----

    fn color_of_line(&self) -> Option<Color> {
        let (w, b) = (self.set.white.is_some(), self.set.black.is_some());
        match (w, b) {
            (false, false) => None,
            (true, false) => Some(Color::White),
            (false, true) => Some(Color::Black),
            (true, true) => match self.config.side {
                TrainSide::White => Some(Color::White),
                TrainSide::Black => Some(Color::Black),
                TrainSide::Both => {
                    let other = match self.first_color {
                        Color::White => Color::Black,
                        Color::Black => Color::White,
                    };
                    Some(if self.line_no.is_multiple_of(2) {
                        self.first_color
                    } else {
                        other
                    })
                }
            },
        }
    }

    /// Draws a new line and shows it (D2214). With `same` the last one is
    /// played again (D2219).
    fn begin_line(&mut self, same: bool) -> Vec<Out> {
        if !same {
            let Some(color) = self.color_of_line() else {
                return self.end_empty();
            };
            let Some(rep) = self.set.for_color(color).cloned() else {
                return self.end_empty();
            };
            let samples = if self.config.no_repeat {
                WALK_SAMPLES_NO_REPEAT
            } else {
                WALK_SAMPLES
            };
            let depth = usize::from(self.config.max_depth);
            let key = self.key_of(color).to_owned();
            let today = self.today;
            let mut rng = self.rng.clone();
            let mut best: Option<Walk> = None;
            let mut repeated = 0usize;
            for _ in 0..samples {
                let need = |hash: u64| need_of(self.store.stat(&key, hash), today);
                let walk = sample_walk(
                    &rep,
                    color,
                    depth,
                    self.config.opponent_best,
                    &mut rng,
                    &need,
                );
                if !has_user_move(&walk, color) {
                    continue;
                }
                if self.config.no_repeat && self.done_lines.contains(&walk.id) {
                    repeated += 1;
                    continue;
                }
                if best.as_ref().is_none_or(|b| walk.need > b.need) {
                    best = Some(walk);
                }
            }
            self.rng = rng;
            let Some(best) = best else {
                if repeated > 0 {
                    self.set_phase(OpeningPhase::Exhausted);
                    return vec![Out::SaveStore];
                }
                return self.end_empty();
            };
            self.color = color;
            self.target = best.moves;
            self.target_id = best.id;
            self.line_no = self.line_no.saturating_add(1);
        }
        self.pos = Position::new();
        self.ply = 0;
        self.on_target = true;
        self.line_errors = 0;
        self.visit = Visit::default();
        self.hint_level = 0;
        self.set_phase(OpeningPhase::UserTurn);
        let mut outs = vec![Out::ShowGame {
            flipped: self.color == Color::Black,
        }];
        outs.extend(self.advance());
        outs
    }

    /// Plays the replies of the machine until the user is to move or the
    /// line ends (D2215, D2219).
    fn advance(&mut self) -> Vec<Out> {
        let mut outs = Vec::new();
        let Some(rep) = self.rep() else {
            outs.extend(self.end_session(true));
            return outs;
        };
        let depth = usize::from(self.config.max_depth);
        loop {
            if self.ply >= depth {
                outs.extend(self.finish_line(LineEnd::Depth));
                return outs;
            }
            let Some(node) = rep.node(&self.pos).filter(|n| !n.moves.is_empty()) else {
                outs.extend(self.finish_line(LineEnd::Complete));
                return outs;
            };
            if self.pos.turn() == self.color {
                self.enter_user_turn();
                return outs;
            }
            let on_target = self
                .on_target
                .then(|| self.target.get(self.ply).copied())
                .flatten()
                .filter(|t| node.moves.iter().any(|m| m.mv == *t));
            let chosen = on_target.or_else(|| {
                self.on_target = false;
                pick_weighted(&node.moves, self.config.opponent_best, &mut self.rng).map(|m| m.mv)
            });
            let Some(mv) = chosen else {
                outs.extend(self.finish_line(LineEnd::Complete));
                return outs;
            };
            if self.pos.play(mv).is_err() {
                outs.extend(self.finish_line(LineEnd::Complete));
                return outs;
            }
            self.ply += 1;
            outs.push(Out::Play(mv));
        }
    }

    fn enter_user_turn(&mut self) {
        self.set_phase(OpeningPhase::UserTurn);
        self.visit = Visit::default();
        self.hint_level = 0;
        let key = self.key_of(self.color).to_owned();
        let hash = self.pos.zobrist().value();
        if let Some(stat) = self.store.stat_mut(&key, hash) {
            stat.seen = stat.seen.saturating_add(1);
        }
        self.used = true;
    }

    fn finish_line(&mut self, end: LineEnd) -> Vec<Out> {
        self.session.lines = self.session.lines.saturating_add(1);
        if self.line_errors == 0 {
            self.session.perfect_lines = self.session.perfect_lines.saturating_add(1);
        }
        self.done_lines.insert(self.target_id);
        self.hint_level = 0;
        self.set_phase(OpeningPhase::LineDone(end));
        vec![Out::SaveStore]
    }

    fn with_stat(&mut self, change: impl FnOnce(&mut PosStat)) {
        let key = self.key_of(self.color).to_owned();
        let hash = self.pos.zobrist().value();
        if let Some(stat) = self.store.stat_mut(&key, hash) {
            change(stat);
        }
        self.used = true;
    }

    /// The user played `mv` (D2216).
    pub fn user_move(&mut self, mv: Move) -> Vec<Out> {
        if self.phase != OpeningPhase::UserTurn {
            return Vec::new();
        }
        let Some(rep) = self.rep() else {
            return Vec::new();
        };
        let entry = rep.find(&self.pos, mv).copied();
        let today = self.today;
        match entry {
            None => {
                self.refuse(OpeningNotice::OutOfRepertoire);
                self.with_stat(|s| {
                    s.out = s.out.saturating_add(1);
                    if s.box_no > 0 {
                        s.demote(today);
                    }
                });
                self.session.out = self.session.out.saturating_add(1);
                Vec::new()
            }
            Some(m) if m.dubious && self.config.strict => {
                self.refuse(OpeningNotice::Dubious);
                self.with_stat(|s| {
                    s.dubious = s.dubious.saturating_add(1);
                    s.demote(today);
                });
                self.session.dubious = self.session.dubious.saturating_add(1);
                Vec::new()
            }
            Some(m) => self.accept(mv, m.dubious),
        }
    }

    fn refuse(&mut self, notice: OpeningNotice) {
        self.visit.errors = self.visit.errors.saturating_add(1);
        self.line_errors = self.line_errors.saturating_add(1);
        self.hint_level = 0;
        self.notice = Some(notice);
    }

    fn accept(&mut self, mv: Move, dubious: bool) -> Vec<Out> {
        let clean = self.visit.errors == 0 && !self.visit.helped && !dubious;
        let today = self.today;
        if clean {
            self.with_stat(|s| {
                s.good = s.good.saturating_add(1);
                s.promote(today);
            });
            self.session.good = self.session.good.saturating_add(1);
        } else {
            if dubious {
                self.line_errors = self.line_errors.saturating_add(1);
                self.session.dubious = self.session.dubious.saturating_add(1);
            }
            self.with_stat(|s| {
                if dubious {
                    s.dubious = s.dubious.saturating_add(1);
                }
                s.demote(today);
            });
        }
        if self.pos.play(mv).is_err() {
            return Vec::new();
        }
        self.on_target = self.on_target && self.target.get(self.ply) == Some(&mv);
        self.ply += 1;
        self.hint_level = 0;
        self.notice = None;
        let mut outs = vec![Out::Play(mv)];
        outs.extend(self.advance());
        outs
    }

    /// The hint button (D2217): realce the origin, then draw the arrow.
    pub fn hint_step(&mut self) {
        if self.phase != OpeningPhase::UserTurn {
            return;
        }
        if self.rep().and_then(|r| r.best(&self.pos)).is_none() {
            self.notice = Some(OpeningNotice::NoHint);
            return;
        }
        self.notice = None;
        self.hint_level = (self.hint_level + 1).min(2);
        self.visit.helped = true;
        // Asking for help spoils the line as much as a mistake (D2222).
        if self.hint_level == 1 {
            self.line_errors = self.line_errors.saturating_add(1);
        }
    }

    /// "Next line" (D2219).
    pub fn next_line(&mut self) -> Vec<Out> {
        match self.phase {
            OpeningPhase::UserTurn | OpeningPhase::LineDone(_) => self.begin_line(false),
            _ => Vec::new(),
        }
    }

    /// "Repeat the line" (D2219).
    pub fn retry(&mut self) -> Vec<Out> {
        match self.phase {
            OpeningPhase::UserTurn | OpeningPhase::LineDone(_) if !self.target.is_empty() => {
                self.begin_line(true)
            }
            _ => Vec::new(),
        }
    }

    /// Shows or hides the report (D2224).
    pub fn toggle_report(&mut self) {
        if self.report.is_some() {
            self.report = None;
            return;
        }
        if !self.is_live() {
            return;
        }
        self.report = Some(self.compute_report());
    }

    fn compute_report(&self) -> ReportView {
        let mut report = ReportView::default();
        for color in [Color::White, Color::Black] {
            let Some(rep) = self.set.for_color(color) else {
                continue;
            };
            let key = self.key_of(color);
            for (hash, _node) in rep.positions_of(color) {
                report.positions = report.positions.saturating_add(1);
                match self.store.stat(key, hash.value()) {
                    Some(stat) if stat.seen > 0 => {
                        report.played = report.played.saturating_add(1);
                        report.good = report.good.saturating_add(stat.good);
                        report.dubious = report.dubious.saturating_add(stat.dubious);
                        report.out = report.out.saturating_add(stat.out);
                        if stat.need(self.today) > 0 {
                            report.due = report.due.saturating_add(1);
                        }
                        if stat.learned() {
                            report.learned = report.learned.saturating_add(1);
                        }
                    }
                    _ => report.never = report.never.saturating_add(1),
                }
            }
        }
        report
    }

    // ---- ending ----

    fn end_session(&mut self, reopen: bool) -> Vec<Out> {
        self.dialog_open = reopen;
        self.pending = None;
        if let Some(cancel) = self.build_cancel.take() {
            cancel.cancel();
        }
        self.set = RepSet::default();
        self.target.clear();
        self.report = None;
        self.visit = Visit::default();
        self.hint_level = 0;
        self.set_phase(OpeningPhase::Closed);
        let mut outs = vec![Out::Closed];
        if self.used {
            outs.push(Out::SaveStore);
        }
        outs
    }

    /// Ends the session because no line can be drawn: the dialog comes back
    /// with the reason (D2258).
    fn end_empty(&mut self) -> Vec<Out> {
        let outs = self.end_session(true);
        self.notice = Some(OpeningNotice::EmptyRepertoire);
        outs
    }

    /// Ends the session because of `notice`, bringing the dialog back to
    /// show it (D2237).
    pub fn abort(&mut self, notice: OpeningNotice) -> Vec<Out> {
        if !self.is_live() {
            return Vec::new();
        }
        let outs = self.end_session(true);
        self.notice = Some(notice);
        outs
    }

    /// Ends the session (the "Close" button).
    pub fn close(&mut self) -> Vec<Out> {
        // A dialog that is open without a session is not this call's to
        // close: the stopping points of the other modes call it too (D2258).
        if !self.is_live() {
            return Vec::new();
        }
        self.dialog_open = false;
        self.end_session(false)
    }

    // ---- time ----

    /// The one-second tick of the front-end (D2231).
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
        if let Some(pending) = self.pending.as_mut() {
            pending.waited_ms = pending.waited_ms.saturating_add(delta);
            if pending.waited_ms >= WATCHDOG_MS {
                let outs = self.end_session(true);
                self.notice = Some(OpeningNotice::TimedOut);
                return outs;
            }
        }
        Vec::new()
    }
}
