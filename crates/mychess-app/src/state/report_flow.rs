//! The game report in the state (spec M4-10b, D1420-D1445).
//!
//! The run is the annotation machine in report mode
//! (`annotate_flow::Active::report`): one search per position of the main
//! line with two lines, the watchdog, the cancel and the engine restore of
//! the annotation, and no edit of the game. When it ends this module turns
//! the evaluations collected into a [`ReportData`].

use super::AppState;
use crate::annotate::{
    AnnotateMessage, AnnotateNotice, Informant, Outcome, PositionEval, apply_actions,
};
use crate::effect::Effect;
use crate::engines::{EngineRunState, RunId};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::prefs::{PrefChange, PrefsMessage};
use crate::report::{
    CLOCK_TICK_SECS, ClassMark, ReportClock, ReportData, ReportDialog, ReportEffort, ReportMessage,
    ReportOrigin, ReportPreset, Review, ReviewWalk, SideFilter, WalkDirection, build_report,
    eta_seconds, evals_from_game, infer_user_side, inputs_from_game_with_book, walk_plan,
    write_plan,
};
use crate::session::{GameMessage, Nav};
use mychess_core::Move;
use std::time::Duration;

/// What [`AppState::report_store`] did.
enum Stored {
    /// No move to judge.
    Nothing,
    /// The new report replaced the old one.
    Replaced,
    /// A longer report of the same game was kept (it has this many moves).
    Kept(u32),
}

impl AppState {
    /// The report of the game in the editor; `None` without one, or when the
    /// main line changed since it was made.
    pub fn report_data(&self) -> Option<&ReportData> {
        self.report
            .data
            .as_ref()
            .filter(|data| data.matches(self.session.game()))
    }

    pub(crate) fn report_message(&mut self, message: ReportMessage, effects: &mut Vec<Effect>) {
        match message {
            // The preset of a run in progress is the one it started with (D1583).
            ReportMessage::SetPreset(preset) => {
                if !self.annotation_running() {
                    self.report.preset = preset;
                    let config = self.prefs.report_config.with_preset(preset);
                    self.report_save_config(config, effects);
                }
            }
            ReportMessage::Generate => self.report_generate(Vec::new(), effects),
            ReportMessage::Continue => self.report_continue(effects),
            ReportMessage::OpenConfig => self.report_open_config(),
            ReportMessage::CloseConfig => self.report.config_dialog = None,
            ReportMessage::ConfigSet(config) => {
                if let Some(dialog) = self.report.config_dialog.as_mut() {
                    dialog.config = config.sanitized();
                }
            }
            ReportMessage::ConfigEngine(engine) => {
                if let Some(dialog) = self.report.config_dialog.as_mut() {
                    dialog.engine = engine;
                }
            }
            ReportMessage::ConfigDefaults => {
                if let Some(dialog) = self.report.config_dialog.as_mut() {
                    dialog.config = crate::report::ReportConfig::default();
                }
            }
            ReportMessage::ConfigStart => {
                if let Some(dialog) = self.report.config_dialog.take() {
                    self.report_save_config(dialog.config, effects);
                    self.report.engine = dialog.engine;
                    self.report_generate(Vec::new(), effects);
                }
            }
            ReportMessage::SetUserSide(side) => {
                self.report.user_side = side;
                self.report.user_side_game = self.session.generation();
            }
            ReportMessage::Cancel => {
                if self.report_running() {
                    self.annotate_message(&AnnotateMessage::Cancel, effects);
                }
            }
            ReportMessage::Clear => {
                if !self.report_running() {
                    self.report.data = None;
                    self.report.review = None;
                    self.report.walk = None;
                    self.report.user_side = None;
                    self.report.stamp = self.report.stamp.wrapping_add(1);
                }
            }
            ReportMessage::WriteToGame => self.report_write(),
            ReportMessage::FromGame => self.report_from_game(),
            ReportMessage::StartReview { side, highlights } => {
                self.review_start(side, highlights, effects);
            }
            ReportMessage::ReviewNext => self.review_step(true, effects),
            ReportMessage::ReviewPrevious => self.review_step(false, effects),
            ReportMessage::ReviewTry => self.review_try(effects),
            ReportMessage::ReviewShowBest => self.review_show_best(effects),
            ReportMessage::ReviewExit => {
                self.report.review = None;
                self.report.walk = None;
            }
            ReportMessage::WalkTick { stamp } => {
                if self.report.walk.is_some_and(|walk| walk.stamp == stamp) {
                    self.review_walk_step(effects);
                } else if self.report.clock.is_some_and(|clock| clock.stamp == stamp) {
                    self.report_clock_tick(stamp, effects);
                }
            }
            ReportMessage::Filter(class) => self.report.filter = class,
            ReportMessage::GoTo(ply) => {
                let Ok(ply) = u32::try_from(ply) else {
                    return;
                };
                effects.extend(super::update(
                    self,
                    Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(ply))),
                ));
            }
        }
    }

    /// Saves the choices of the report in the preferences (D2100).
    fn report_save_config(
        &mut self,
        config: crate::report::ReportConfig,
        effects: &mut Vec<Effect>,
    ) {
        self.prefs_message(
            PrefsMessage::Set(PrefChange::ReportConfig(Box::new(config))),
            effects,
        );
    }

    /// Opens the "Configure report" dialog on the saved choices (D2100).
    fn report_open_config(&mut self) {
        if self.assist_locked() {
            self.notice = Some(AppNotice::Online(
                crate::online::OnlineNotice::AssistanceDisabled,
            ));
            return;
        }
        if self.annotation_running() || self.modal_open() {
            return;
        }
        self.report.config_dialog = Some(ReportDialog {
            config: *self.prefs.report_config,
            engine: self.report.engine,
        });
    }

    /// Goes on with a report that stopped before the end of the game: only
    /// the positions it did not reach are analysed (D1443, D2110). The run
    /// keeps the limit of the first one, so the evaluations are comparable;
    /// the engine too, and when it is gone the current one takes over with
    /// a notice (D2190).
    fn report_continue(&mut self, effects: &mut Vec<Effect>) {
        let (known, effort) = match self.report_data() {
            Some(data) if data.resumable() => match data.effort() {
                Some(effort) => (data.evals.clone(), effort),
                None => return,
            },
            _ => return,
        };
        let mut config = *self.prefs.report_config;
        config.by_time = effort.by_time;
        config.depth = effort.depth;
        config.movetime_ms = effort.movetime_ms;
        let alive = effort.engine.is_some_and(|id| self.engine_is_free(id));
        let wanted = if alive {
            effort.engine
        } else {
            self.report.engine
        };
        self.report_launch(known, config, wanted, effects);
        if !alive && self.report_running() {
            self.notice = Some(AppNotice::Annotate(
                AnnotateNotice::ReportContinuedOtherEngine,
            ));
        }
    }

    /// Whether the engine run exists, runs and is not playing.
    fn engine_is_free(&self, id: RunId) -> bool {
        self.engine_runs
            .get(id)
            .is_some_and(|r| r.state == EngineRunState::Running && !self.play_uses(r.id))
    }

    /// Starts the run over the whole main line with the engine of the
    /// dialog, or the default one. `known` are the evaluations of a stopped
    /// report that the run keeps (empty: from the start).
    fn report_generate(&mut self, known: Vec<PositionEval>, effects: &mut Vec<Effect>) {
        let config = *self.prefs.report_config;
        if !config.by_time {
            self.report.preset = ReportPreset::nearest(config.depth);
        }
        let wanted = self.report.engine;
        self.report_launch(known, config, wanted, effects);
    }

    /// The start of a run with `config` limits and `wanted` engine.
    fn report_launch(
        &mut self,
        known: Vec<PositionEval>,
        config: crate::report::ReportConfig,
        wanted: Option<RunId>,
        effects: &mut Vec<Effect>,
    ) {
        // The report is help: closed during an online game (D1441).
        if self.assist_locked() {
            self.notice = Some(AppNotice::Online(
                crate::online::OnlineNotice::AssistanceDisabled,
            ));
            return;
        }
        if !self.annotate_preflight() {
            return;
        }
        let base_done = u32::try_from(known.len().saturating_sub(1)).unwrap_or(u32::MAX);
        let options = config.options();
        self.annotate_launch(options, Informant::default(), wanted, Some(known), effects);
        // The review in progress is closed only by a run that began (D2198).
        if self.report_running() {
            self.report.review = None;
        }
        self.report_clock_start(base_done, effects);
    }

    // ---- the clock of the run (D2121) ----

    /// Starts counting the seconds of a run that began.
    fn report_clock_start(&mut self, base_done: u32, effects: &mut Vec<Effect>) {
        self.report.clock = None;
        if !self.report_running() {
            return;
        }
        let stamp = self.new_request().0;
        self.report.clock = Some(ReportClock {
            stamp,
            seconds: 0,
            base_done,
        });
        effects.push(Self::clock_tick(stamp));
    }

    fn clock_tick(stamp: u64) -> Effect {
        Effect::ScheduleReviewTick {
            after: Duration::from_secs(u64::from(CLOCK_TICK_SECS)),
            stamp,
        }
    }

    /// One second of the run went by; the clock dies with the run.
    fn report_clock_tick(&mut self, stamp: u64, effects: &mut Vec<Effect>) {
        if !self.report_running() {
            self.report.clock = None;
            return;
        }
        if let Some(clock) = self.report.clock.as_mut() {
            clock.seconds = clock.seconds.saturating_add(CLOCK_TICK_SECS);
            effects.push(Self::clock_tick(stamp));
        }
    }

    /// The seconds the run in progress still needs, from the pace of the
    /// moves done since it began (D2121); `None` before the first move.
    pub fn report_eta_seconds(&self) -> Option<u32> {
        let clock = self.report.clock?;
        if !self.report_running() {
            return None;
        }
        let progress = self.annotation_progress()?;
        let done_now = progress.done_plies.saturating_sub(clock.base_done);
        let left = progress.total_plies.saturating_sub(progress.done_plies);
        eta_seconds(clock.seconds, done_now, left)
    }

    /// Drops the review when the report it walks no longer describes the
    /// game (another game, an edited main line, a cleared report).
    pub(crate) fn report_guard(&mut self, effects: &mut Vec<Effect>) {
        // The clock dies with the run (D2121).
        if self.report.clock.is_some() && !self.report_running() {
            self.report.clock = None;
        }
        if self.report.review.is_some() && self.report_data().is_none() {
            self.report.review = None;
        }
        // The watchdog of the walk (D1666): it lives only while the review
        // it walks for lives, on the moment it walks to.
        if let Some(walk) = self.report.walk {
            let target = self
                .report
                .review
                .as_ref()
                .and_then(Review::current)
                .map(|moment| u32::try_from(moment.ply.saturating_sub(1)).unwrap_or(0));
            if target != Some(walk.target_ply) {
                self.report.walk = None;
            } else if self.modal_open() || self.annotation_running() || self.play_live() {
                // Something else took the board: the walk ends at its
                // moment, never in between (D1680).
                self.review_walk_finish(effects);
            }
        }
    }

    /// Whether `←`/`→` visit the moments of the review now (D2118): the
    /// review is on a moment (not trying, which needs the board for the
    /// moves), nothing modal is open and the cursor is where the review put
    /// it. Anywhere else the arrows walk the game as always.
    pub fn review_owns_arrows(&self) -> bool {
        let Some(review) = self.report.review.as_ref() else {
            return false;
        };
        if review.step == crate::report::ReviewStep::Trying
            || self.modal_open()
            || self.annotation_running()
            || self.play_live()
            || self.assist_locked()
        {
            return false;
        }
        let Some(moment) = review.current() else {
            return false;
        };
        self.report_data().is_some()
            && ReportData::node_before(self.session.game(), moment.ply)
                == Some(self.session.cursor())
    }

    /// The class to draw on the destination square of the move under the
    /// cursor (D1434, D2111): the report must describe the game, the mark be
    /// on, and no review, walk or run in the way (the review draws its own
    /// arrow; the mark would spoil "Try again").
    pub fn board_class_mark(&self) -> Option<ClassMark> {
        if !self.prefs.report_config.board_marks
            || self.report.review.is_some()
            || self.report.walk.is_some()
            || self.annotation_running()
            || self.assist_locked()
        {
            return None;
        }
        let data = self.report_data()?;
        let ply = usize::try_from(self.cursor_mainline_ply()?).ok()?;
        let judged = data.report.moves.get(ply.checked_sub(1)?)?;
        Some(ClassMark {
            square: judged.played.to()?,
            class: judged.class,
        })
    }

    /// Whether the cursor is walking to the moment of the review.
    pub fn review_walking(&self) -> bool {
        self.report.walk.is_some()
    }

    /// The longest a slide of the walk may take, in milliseconds: the board
    /// slides each piece in at most the interval of the steps (D1662).
    pub fn review_walk_slide_cap_ms(&self) -> Option<u32> {
        self.report.walk.map(|walk| walk.interval_ms)
    }

    /// The arrows for the board now: those of the guided review while it
    /// shows one (D1664, D1665), else those of the engine (the review
    /// suppresses the live ones, so the two never mix).
    pub fn board_arrows(&self) -> &[crate::evalbar::EngineArrow] {
        if self.report.arrows.is_empty() {
            self.engine_arrows()
        } else {
            &self.report.arrows
        }
    }

    /// Any action of the user finishes the walk on the spot (D1663): the
    /// board is never left on a position in between.
    pub(crate) fn review_walk_finish(&mut self, effects: &mut Vec<Effect>) {
        if let Some(walk) = self.report.walk.take() {
            self.walk_navigate(walk.target_ply, effects);
        }
    }

    /// Moves the cursor along the main line straight in the session: the
    /// walk is not an edit of the user, so the locks of a dialog, of the
    /// annotation or of a game against an engine (which a `Navigate` would
    /// pause) do not apply to it (D1680).
    fn walk_navigate(&mut self, ply: u32, effects: &mut Vec<Effect>) {
        effects.extend(
            self.session
                .update(GameMessage::Navigate(Nav::GoToMainlinePly(ply))),
        );
        self.take_session_analysis_notice();
    }

    /// The main-line ply of the cursor; `None` inside a variation.
    pub(crate) fn cursor_mainline_ply(&self) -> Option<u32> {
        let game = self.session.game();
        let cursor = self.session.cursor();
        if cursor == game.root() {
            return Some(0);
        }
        let index = game.mainline().position(|node| node == cursor)?;
        u32::try_from(index.checked_add(1)?).ok()
    }

    /// One step of the walk; schedules the next one unless it arrived.
    fn review_walk_step(&mut self, effects: &mut Vec<Effect>) {
        // Taken while it navigates: the walk is not an action of the user.
        let Some(walk) = self.report.walk.take() else {
            return;
        };
        self.walk_navigate(walk.next_ply, effects);
        if walk.next_ply == walk.target_ply {
            return;
        }
        let next_ply = match walk.direction {
            WalkDirection::Forward => walk.next_ply.saturating_add(1),
            WalkDirection::Backward => walk.next_ply.saturating_sub(1),
        };
        self.report.walk = Some(ReviewWalk { next_ply, ..walk });
        effects.push(Effect::ScheduleReviewTick {
            after: Duration::from_millis(u64::from(walk.interval_ms)),
            stamp: walk.stamp,
        });
    }

    /// Builds and keeps the report of `evals` for the game in the editor.
    /// A report of the same game that judged more moves is kept instead of a
    /// shorter one (D1584).
    fn report_store(&mut self, evals: Vec<PositionEval>, origin: ReportOrigin) -> Stored {
        let thresholds = self.prefs.report_config.thresholds();
        let game = self.session.game();
        let inputs = inputs_from_game_with_book(
            game,
            &evals,
            self.eco_book.as_deref(),
            self.opening_probe(),
        );
        if inputs.moves.is_empty() {
            return Stored::Nothing;
        }
        if let Some(old) = self.report_data()
            && old.report.moves.len() > inputs.moves.len()
        {
            return Stored::Kept(u32::try_from(old.report.moves.len()).unwrap_or(u32::MAX));
        }
        let report = build_report(&inputs.moves, &thresholds);
        self.report.data = Some(ReportData::new(
            game,
            report,
            inputs.sans,
            evals,
            self.report.preset,
            thresholds,
        ));
        if let ReportOrigin::Engine(effort) = origin
            && let Some(data) = self.report.data.take()
        {
            self.report.data = Some(data.with_effort(effort));
        }
        self.report.stamp = self.report.stamp.wrapping_add(1);
        self.report.review = None;
        // The side of the user comes from the names of the game when the
        // preferences know who the user is (D2115).
        // A side chosen for another game does not carry over (D2196).
        if self.report.user_side_game != self.session.generation() {
            self.report.user_side = None;
            self.report.user_side_game = self.session.generation();
        }
        let names = [
            self.prefs.play.player_name.as_str(),
            self.prefs.online.login.as_str(),
        ];
        if let Some(side) = infer_user_side(self.session.game(), &names) {
            self.report.user_side = Some(side);
        }
        Stored::Replaced
    }

    /// Writes the report into the game as one undo point (D1447).
    fn report_write(&mut self) {
        if self.refuse_edit() {
            return;
        }
        let Some(data) = self
            .report
            .data
            .as_ref()
            .filter(|d| d.matches(self.session.game()))
        else {
            return;
        };
        let (actions, moves) = write_plan(self.session.game(), &data.report, &data.evals);
        // Nothing to change: no undo point either (D1585).
        if actions.is_empty() {
            self.notice = Some(AppNotice::Annotate(AnnotateNotice::ReportWritten {
                moves: 0,
                variations: 0,
            }));
            return;
        }
        self.notice = Some(AppNotice::Annotate(
            match self.session.edit(|game, _| apply_actions(game, &actions)) {
                Ok(stats) => AnnotateNotice::ReportWritten {
                    moves,
                    variations: stats.variations,
                },
                Err(_) => AnnotateNotice::ReportWriteFailed,
            },
        ));
    }

    /// Rebuilds the report from the `[%eval]` comments of the game, with no
    /// engine (D1439).
    fn report_from_game(&mut self) {
        if self.annotation_running() {
            self.notice = Some(AppNotice::Annotate(AnnotateNotice::Busy));
            return;
        }
        let evals = evals_from_game(self.session.game());
        match self.report_store(evals, ReportOrigin::Game) {
            Stored::Nothing => {
                self.notice = Some(AppNotice::Annotate(AnnotateNotice::ReportNoEvals));
            }
            Stored::Kept(kept) => self.report_kept(kept, 0),
            Stored::Replaced => {}
        }
    }

    // ---- the guided review (M4-10c) ----

    /// Puts the cursor on the position before the move of the moment on
    /// show.
    fn review_goto(&mut self, effects: &mut Vec<Effect>) {
        let Some(moment) = self.report.review.as_ref().and_then(Review::current) else {
            return;
        };
        let target = u32::try_from(moment.ply.saturating_sub(1)).unwrap_or(0);
        // A new destination replaces the walk in progress (its timer is now
        // stale).
        self.report.walk = None;
        // An autoplay would fight the walk for the cursor (D1681).
        effects.extend(self.session.update(GameMessage::StopAutoplay));
        let Some(from) = self.cursor_mainline_ply() else {
            // Inside a variation there is no way along the main line.
            self.walk_navigate(target, effects);
            return;
        };
        let plan = walk_plan(from, target, self.prefs.board.animation_ms);
        if let Some(jump) = plan.jump_to {
            self.walk_navigate(jump, effects);
        }
        if plan.steps == 0 {
            return;
        }
        let (direction, first) = if target >= from {
            (
                WalkDirection::Forward,
                plan.jump_to.unwrap_or(from).saturating_add(1),
            )
        } else {
            (
                WalkDirection::Backward,
                plan.jump_to.unwrap_or(from).saturating_sub(1),
            )
        };
        let walk = ReviewWalk {
            target_ply: target,
            next_ply: first,
            direction,
            interval_ms: plan.interval_ms,
            stamp: self.new_request().0,
        };
        self.report.walk = Some(walk);
        if plan.jump_to.is_some() {
            // Let the arrival of the jump be seen before the steps start.
            effects.push(Effect::ScheduleReviewTick {
                after: Duration::from_millis(u64::from(walk.interval_ms)),
                stamp: walk.stamp,
            });
        } else {
            self.review_walk_step(effects);
        }
    }

    fn review_start(&mut self, side: SideFilter, highlights: bool, effects: &mut Vec<Effect>) {
        if self.edits_blocked() {
            return;
        }
        // The review shows the game of the user: the tactics training ends
        // first and gives it back (D1964).
        self.tactics_halt();
        let Some(data) = self.report_data() else {
            return;
        };
        self.report.review = Review::new(&data.report, side, highlights);
        self.review_goto(effects);
    }

    fn review_step(&mut self, forward: bool, effects: &mut Vec<Effect>) {
        let moved = match self.report.review.as_mut() {
            Some(review) if forward => review.advance(),
            Some(review) => review.previous(),
            None => false,
        };
        if moved {
            self.review_goto(effects);
        }
    }

    /// The engine's best move at the moment on show.
    fn review_best(&self) -> Option<Move> {
        let ply = self.report.review.as_ref()?.current()?.ply;
        self.report_data()?
            .report
            .moves
            .get(ply.checked_sub(1)?)?
            .best
    }

    fn review_try(&mut self, effects: &mut Vec<Effect>) {
        if self.edits_blocked() || self.review_best().is_none() {
            return;
        }
        if let Some(review) = self.report.review.as_mut() {
            review.start_trying();
        }
        self.review_goto(effects);
    }

    fn review_show_best(&mut self, effects: &mut Vec<Effect>) {
        if self.edits_blocked() {
            return;
        }
        if let Some(review) = self.report.review.as_mut() {
            review.reveal();
        }
        self.review_goto(effects);
    }

    /// Whether the board is at the position before the moment on show while
    /// the user looks for the better move (the only place the interceptor
    /// works; elsewhere the board is free).
    pub(crate) fn review_trying_here(&self) -> bool {
        let Some(review) = self.report.review.as_ref() else {
            return false;
        };
        if review.step != crate::report::ReviewStep::Trying {
            return false;
        }
        let Some(ply) = review.current().map(|m| m.ply) else {
            return false;
        };
        let Some(before) = ReportData::node_before(self.session.game(), ply) else {
            return false;
        };
        self.session.cursor() == before && self.report_data().is_some()
    }

    /// While the user looks for the better move, every message that would put
    /// a move in the game is judged instead (D1425, D1580): the session
    /// resolves the message (squares, promotion question, typed text, key
    /// entry, null move) with its capture mode on and the move it found is
    /// the attempt. A message that cannot be resolved is still consumed (the
    /// session leaves its own remark); a message that adds moves some other
    /// way is consumed with a remark. Returns whether the message was
    /// consumed; it is `true` for every move message while trying.
    pub(crate) fn review_intercept(
        &mut self,
        message: &GameMessage,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if !self.review_trying_here() {
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
            GameMessage::StartVariation
            | GameMessage::AnswerReplace(_)
            | GameMessage::AddEngineLine { .. }
            | GameMessage::AddEngineLines { .. } => {
                self.notice = Some(AppNotice::Annotate(AnnotateNotice::ReviewMoveIgnored));
                return true;
            }
            _ => return false,
        }
        let (session_effects, played) = self.session.update_capturing(message.clone());
        effects.extend(session_effects);
        if let Some(played) = played {
            let best = self.review_best();
            let judged = self
                .report
                .review
                .as_mut()
                .and_then(|review| review.judge(played, best));
            if judged.is_none() {
                self.notice = Some(AppNotice::Annotate(AnnotateNotice::ReviewMoveIgnored));
            }
        }
        true
    }

    fn report_kept(&mut self, kept: u32, plies: u32) {
        self.notice = Some(AppNotice::Annotate(AnnotateNotice::ReportKept {
            kept,
            plies,
        }));
    }

    /// The run ended: keeps what it analysed as the report (a partial one
    /// after a cancel or a failure, D1443) and says why it stopped.
    pub(crate) fn report_run_ended(
        &mut self,
        outcome: Outcome,
        evals: Vec<PositionEval>,
        total: u32,
        effort: ReportEffort,
    ) {
        let plies = u32::try_from(evals.len().saturating_sub(1)).unwrap_or(u32::MAX);
        self.report.clock = None;
        // The game moved under the run: what it saw is not the game.
        if outcome == Outcome::GameChanged {
            self.notice = Some(AppNotice::Annotate(AnnotateNotice::GameChanged { plies }));
            return;
        }
        if let Stored::Kept(kept) = self.report_store(evals, ReportOrigin::Engine(effort)) {
            self.report_kept(kept, plies);
            return;
        }
        if outcome != Outcome::Finished {
            self.notice = Some(AppNotice::Annotate(AnnotateNotice::ReportStopped {
                plies,
                total,
            }));
        }
    }
}
