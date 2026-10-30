//! The analysis window in the state: the reports of the engines, the buttons
//! of a section and the position that is sent (spec M4-4).
//!
//! Pure bookkeeping, like the other flows: the engine commands, the log
//! reads and the file writes are [`Effect`]s. Nothing here walks the game
//! or formats text per report beyond what the merge needs (rule 16).

use std::sync::Arc;

use mychess_core::NodeId;
use mychess_engine::{BestMove, EngineCommand, Protocol, SearchId, SearchInfo};
use mychess_engine::{SearchPosition, SearchRequest};

use super::AppState;
use crate::analysis::{
    AllLinesMode, AnalysisMessage, AnalysisNotice, AnalysisView, Binding, ExcludeError,
    FormatOptions, LockedPosition, NumberStyle, PvLine, Restart, format_comment, parse_exclusion,
    position_of, search_moves_without,
};
use crate::effect::Effect;
use crate::engines::{AnalysisTarget, EngineRunState, RunId};
use crate::files::GameOrigin;
use crate::flow::AppNotice;
use crate::session::{EngineLineToAdd, GameMessage, PlayOptions};

/// Which lines an "add" button takes.
enum Pick {
    /// The line with this number.
    Number(u8),
    /// The lines of "add all" in this mode.
    All(AllLinesMode),
}

impl AppState {
    /// What the window shows for an execution.
    pub fn analysis_view(&self, run: RunId) -> Option<&AnalysisView> {
        self.analysis.get(run)
    }

    /// Copies the remark that adding an engine line left in the session to
    /// the status bar. Called after every game message.
    pub(crate) fn take_session_analysis_notice(&mut self) {
        if let Some(notice) = self.session.analysis_notice().cloned() {
            self.notice = Some(AppNotice::Analysis(notice));
        }
    }

    fn notice_analysis(&mut self, notice: AnalysisNotice) {
        self.notice = Some(AppNotice::Analysis(notice));
    }

    fn analysis_options(&self) -> FormatOptions {
        FormatOptions {
            numbers: NumberStyle::from(self.prefs.notation.move_number_space),
            max_ply: usize::try_from(self.prefs.analysis.max_ply).unwrap_or(usize::MAX),
        }
    }

    /// Keeps one view per execution and the text of the lines in step with
    /// the notation options. Cheap when nothing changed: it runs after every
    /// update, including each report of an engine.
    pub(crate) fn refresh_analysis(&mut self) {
        if self.engine_runs.is_empty() && self.analysis.as_slice().is_empty() {
            return;
        }
        let runs = &self.engine_runs;
        self.analysis.retain(|id| runs.get(id).is_some());
        self.analysis
            .restarts
            .retain(|(id, _)| runs.get(*id).is_some());
        for run in self.engine_runs.as_slice() {
            if self.analysis.get(run.id).is_none() {
                let multipv = self.engines.get(run.engine).map_or(1, |e| e.multipv);
                self.analysis
                    .push(AnalysisView::new(run.id, multipv, &self.prefs.analysis));
            }
        }
        let options = self.analysis_options();
        for view in self.analysis.iter_mut() {
            view.reformat(options);
        }
    }

    // ---- events of the engines ----

    pub(crate) fn analysis_info(&mut self, run: RunId, info: SearchInfo) {
        let options = self.analysis_options();
        if let Some(view) = self.analysis.get_mut(run) {
            view.apply_info(info, options);
        }
    }

    /// The engine says which request starts as `search` (D780).
    pub(crate) fn analysis_search_tagged(&mut self, run: RunId, search: SearchId, tag: u64) {
        if let Some(view) = self.analysis.get_mut(run) {
            view.announce(search, tag);
        }
    }

    pub(crate) fn analysis_search_started(&mut self, run: RunId, id: SearchId) {
        // The position travels with the search (D780). Only an engine that
        // sends no label falls back on the last position sent.
        let fallback = self
            .engine_runs
            .get(run)
            .and_then(|r| r.sent.as_ref().map(|(target, _)| Arc::clone(target)));
        if let Some(view) = self.analysis.get_mut(run) {
            let target = match view.take_announced(id) {
                Binding::Missing => fallback,
                Binding::Unknown => None,
                Binding::Target(target) => Some(target),
            };
            view.begin_search(id, target);
        }
    }

    pub(crate) fn analysis_best_move(&mut self, run: RunId, best: BestMove) {
        if let Some(view) = self.analysis.get_mut(run) {
            view.finish(best);
        }
    }

    // ---- the position sent ----

    fn engine_command(&self, run: RunId, command: EngineCommand, effects: &mut Vec<Effect>) {
        effects.push(Effect::EngineCommand {
            run,
            command: Box::new(command),
            hub: self.engine_hub.clone(),
        });
    }

    /// Sends the search of `target` to `run`: the excluded moves as
    /// `searchmoves` (an exclusion made for another position is dropped
    /// first, rule 10), nothing at all for a position without moves (D450).
    pub(crate) fn send_search(
        &mut self,
        run: RunId,
        target: &Arc<AnalysisTarget>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(position) = position_of(target) else {
            return;
        };
        let no_moves = position.legal_moves().is_empty();
        let Some(view) = self.analysis.get_mut(run) else {
            return;
        };
        if !view.exclusion.is_empty() && view.exclusion_target.as_ref() != Some(target) {
            view.clear_exclusion();
        }
        if no_moves {
            // Also a search that was sent but has not started yet (D780).
            let searching = view.search_sent;
            view.set_terminal(target, &position);
            if searching {
                self.engine_command(run, EngineCommand::Stop, effects);
            }
            return;
        }
        view.clear_terminal();
        let mut request = SearchRequest::infinite(SearchPosition {
            start: target.start.clone(),
            moves: target.moves.clone(),
        });
        request.tag = view.next_tag(target);
        request.search_moves.clone_from(&view.search_moves);
        self.engine_command(run, EngineCommand::Search(request), effects);
    }

    /// An execution ended: if it was being restarted, the new one starts
    /// with the settings of the section (D446).
    pub(crate) fn analysis_run_stopped(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let Some(at) = self.analysis.restarts.iter().position(|(r, _)| *r == run) else {
            return;
        };
        let (_, restart) = self.analysis.restarts.remove(at);
        let Some(entry) = self.engines.get(restart.engine).cloned() else {
            return;
        };
        let mut defaults = self.prefs.analysis.run_defaults();
        defaults.low_priority = restart.low_priority;
        let multipv = entry.multipv;
        let new = self.begin_run(entry, restart.analyzing, defaults, effects);
        let mut view = restart.view.for_new_run(new);
        // The number of lines the list holds now, not the one at the switch.
        view.set_multipv(multipv);
        self.analysis.push(view);
    }

    // ---- the messages ----

    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    pub(crate) fn analysis_message(&mut self, message: AnalysisMessage, effects: &mut Vec<Effect>) {
        match message {
            AnalysisMessage::Toggle(run) => self.analysis_toggle(run, effects),
            AnalysisMessage::AddMove(run) => self.analysis_add_move(run, effects),
            AnalysisMessage::AddScore(run) => self.analysis_add_score(run, effects),
            AnalysisMessage::AddLine(run) => {
                self.analysis_add_lines(run, &Pick::Number(1), effects);
            }
            AnalysisMessage::AddLineNumber(run, n) => {
                self.analysis_add_lines(run, &Pick::Number(n), effects);
            }
            AnalysisMessage::AddAllLines(run, mode) => {
                self.analysis_add_lines(run, &Pick::All(mode), effects);
            }
            AnalysisMessage::SetMultiPv(run, n) => self.analysis_set_multipv(run, n, effects),
            AnalysisMessage::ToggleLock(run) => self.analysis_toggle_lock(run),
            AnalysisMessage::OpenExclude(run) => {
                if !self.training_uses(run)
                    && self.is_running(run)
                    && self.edit_allowed(run)
                    && let Some(view) = self.analysis.get_mut(run)
                {
                    view.exclude_open = true;
                }
            }
            AnalysisMessage::CloseExclude(run) => {
                if let Some(view) = self.analysis.get_mut(run) {
                    view.exclude_open = false;
                }
            }
            AnalysisMessage::SetExclusion(run, text) => self.analysis_set_exclusion(run, &text),
            AnalysisMessage::ToggleLowPriority(run) => self.analysis_toggle_priority(run, effects),
            AnalysisMessage::ToggleInfo(run) => {
                self.edit_settings(run, |view| view.show_info = !view.show_info);
            }
            AnalysisMessage::SetDisplay(run, display) => {
                self.edit_settings(run, |view| view.display = display);
            }
            AnalysisMessage::CycleDisplay(run) => {
                self.edit_settings(run, |view| view.display = view.display.next());
            }
            AnalysisMessage::RequestStats(run) => {
                let xboard = self
                    .engine_runs
                    .get(run)
                    .and_then(|r| self.engines.get(r.engine))
                    .is_some_and(|e| e.protocol == Protocol::XBoard);
                if xboard {
                    self.engine_command(run, EngineCommand::RequestStats, effects);
                }
            }
            AnalysisMessage::OpenLog(run) => self.analysis_open_log(run, effects),
            AnalysisMessage::CloseLog(run) => {
                if let Some(view) = self.analysis.get_mut(run) {
                    view.log_open = false;
                    view.log.clear();
                    view.log_next = 0;
                    view.log_in_flight = false;
                }
            }
            AnalysisMessage::LogTick(run) => self.analysis_log_tick(run, effects),
            AnalysisMessage::SetLogFilter(run, text) => {
                self.edit_settings(run, |view| view.log_filter.clone_from(&text));
            }
            AnalysisMessage::SaveLog(run, path) => effects.push(Effect::SaveEngineLog {
                run,
                path,
                hub: self.engine_hub.clone(),
            }),
            AnalysisMessage::Close(run) => {
                self.analysis.restarts.retain(|(r, _)| *r != run);
                self.stop_run(run, effects);
            }
            AnalysisMessage::LogRead {
                run,
                since,
                entries,
                next,
            } => self.analysis_log_read(run, since, entries, next),
            AnalysisMessage::LogSaved { result, .. } => match result {
                Ok(()) => self.notice_analysis(AnalysisNotice::LogSaved),
                Err(error) => self.notice = Some(AppNotice::File(error)),
            },
        }
    }

    /// Changes a setting of the window. During a restart the view that will
    /// replace this one gets the change too, or it would be lost (D446).
    fn edit_settings(&mut self, run: RunId, change: impl Fn(&mut AnalysisView)) {
        if let Some(view) = self.analysis.get_mut(run) {
            change(view);
        }
        if let Some((_, restart)) = self.analysis.restarts.iter_mut().find(|(r, _)| *r == run) {
            change(&mut restart.view);
        }
    }

    /// Whether the buttons that change the game or the search of `run` may
    /// act; a locked analysis refuses them (D444).
    pub(super) fn edit_allowed(&mut self, run: RunId) -> bool {
        if self.engine_runs.get(run).is_some_and(|r| r.locked) {
            self.notice_analysis(AnalysisNotice::Locked);
            return false;
        }
        true
    }

    fn is_running(&self, run: RunId) -> bool {
        self.engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running)
    }

    // ---- start, stop, lock ----

    pub(super) fn analysis_toggle(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if !self.is_running(run) || self.annotation_uses(run) || self.training_uses(run) {
            return;
        }
        let generation = self.session.generation();
        let locked = self
            .analysis
            .get(run)
            .and_then(|v| v.lock.as_ref().map(|l| Arc::clone(&l.target)));
        let Some(r) = self.engine_runs.get_mut(run) else {
            return;
        };
        r.analyzing = !r.analyzing;
        if !r.analyzing {
            r.sent = None;
            self.engine_command(run, EngineCommand::Stop, effects);
            if let Some(view) = self.analysis.get_mut(run) {
                view.clear_exclusion();
                view.search_sent = false;
            }
        } else if let Some(target) = locked {
            // A locked analysis does not follow the board: it searches the
            // position it is locked on.
            r.sent = Some((Arc::clone(&target), generation));
            r.seen_game = Some(generation);
            self.send_search(run, &target, effects);
        }
        // Not locked: `refresh_engines` sends the position now shown.
    }

    fn analysis_toggle_lock(&mut self, run: RunId) {
        if !self.is_running(run) || self.training_uses(run) {
            return;
        }
        if self.engine_runs.get(run).is_some_and(|r| r.locked) {
            self.engine_runs.set_locked(run, false);
            if let Some(view) = self.analysis.get_mut(run) {
                view.lock = None;
                view.clear_lines();
            }
            return;
        }
        let Some(target) = self.analysis_target() else {
            return;
        };
        let lock = self.locked_position(&target);
        self.engine_runs.set_locked(run, true);
        if let Some(view) = self.analysis.get_mut(run) {
            if !view.is_about(&target) {
                view.clear_lines();
            }
            view.lock = Some(lock);
        }
    }

    fn locked_position(&self, target: &Arc<AnalysisTarget>) -> LockedPosition {
        let game = self.session.game();
        let cursor = self.session.cursor();
        let position = self.session.position();
        let last_move = game.parent(cursor).ok().flatten().and_then(|parent| {
            let mv = game.mv(cursor).ok()?;
            game.position_at(parent).ok()?.san(mv).ok()
        });
        let variation_depth = game.path_to(cursor).map_or(0, |path| {
            let heads = path
                .iter()
                .filter(|node| game.is_variation(**node) == Ok(true))
                .count();
            u32::try_from(heads).unwrap_or(u32::MAX)
        });
        let (source, game_number) = match &self.origin {
            GameOrigin::PgnFile { path, index, .. } => (file_name(path), Some(index + 1)),
            GameOrigin::SavedFile { path } => (file_name(path), None),
            _ => (None, None),
        };
        LockedPosition {
            target: Arc::clone(target),
            move_number: position.fullmove_number(),
            side: position.turn(),
            last_move,
            variation_depth,
            source,
            game_number,
        }
    }

    // ---- lines, exclusion, priority ----

    fn analysis_set_multipv(&mut self, run: RunId, n: u8, effects: &mut Vec<Effect>) {
        // The engine of a shoot-out or an automove has a search of its own:
        // changing the lines would queue the board's request again (D852).
        if self.training_uses(run) {
            return;
        }
        if !self.is_running(run) || !self.edit_allowed(run) {
            return;
        }
        let range = self
            .engine_runs
            .get(run)
            .and_then(|r| r.identity.as_ref())
            .and_then(|i| i.capabilities.multipv.clone());
        let Some(range) = range else {
            return self.notice_analysis(AnalysisNotice::MultiPvUnsupported);
        };
        let n = n.clamp(*range.start(), *range.end());
        let Some(view) = self.analysis.get_mut(run) else {
            return;
        };
        if view.multipv == n {
            return;
        }
        view.set_multipv(n);
        self.engine_command(run, EngineCommand::SetMultiPv(n), effects);
        // The number comes back to the list (D426).
        let engine = self.engine_runs.get(run).map(|r| r.engine);
        if let Some(entry) = engine.and_then(|id| self.engines.get(id)) {
            let id = entry.id;
            let mut draft = entry.to_draft();
            draft.multipv = n;
            if self.engines.edit(id, draft).is_ok() {
                self.request_engines_save(effects);
            }
        }
    }

    fn analysis_set_exclusion(&mut self, run: RunId, text: &str) {
        if self.training_uses(run) || !self.is_running(run) || !self.edit_allowed(run) {
            return;
        }
        let Some(r) = self.engine_runs.get(run) else {
            return;
        };
        if !r.analyzing {
            return;
        }
        let supported = r
            .identity
            .as_ref()
            .is_some_and(|i| i.capabilities.search_moves);
        if !supported {
            return self.notice_analysis(AnalysisNotice::ExcludeUnsupported);
        }
        let Some(target) = self.analysis_target() else {
            return;
        };
        let position = self.session.position();
        let excluded = match parse_exclusion(position, text) {
            Ok(excluded) => excluded,
            Err(ExcludeError::Invalid(token)) => {
                return self.notice_analysis(AnalysisNotice::ExcludeInvalid(token));
            }
            Err(_) => return self.notice_analysis(AnalysisNotice::ExcludeAll),
        };
        let san: Vec<String> = excluded
            .iter()
            .filter_map(|m| position.san(*m).ok())
            .collect();
        let search_moves = if excluded.is_empty() {
            Vec::new()
        } else {
            search_moves_without(position, &excluded)
        };
        if let Some(view) = self.analysis.get_mut(run) {
            if excluded.is_empty() {
                view.clear_exclusion();
            } else {
                view.set_exclusion(&target, excluded, san, search_moves);
            }
            view.exclude_open = false;
            // Lines found with the old exclusion may hold the move now
            // excluded (or lack the one now allowed).
            view.clear_lines();
        }
        // The next update sends the search again, with the new moves.
        if let Some(r) = self.engine_runs.get_mut(run) {
            r.sent = None;
        }
    }

    fn analysis_toggle_priority(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let Some(r) = self.engine_runs.get(run) else {
            return;
        };
        if !matches!(r.state, EngineRunState::Starting | EngineRunState::Running) {
            return;
        }
        let (engine, analyzing, low_priority) = (r.engine, r.analyzing, !r.low_priority);
        let Some(view) = self.analysis.get_mut(run) else {
            return;
        };
        view.restarting = true;
        let carried = Box::new(view.clone());
        self.analysis.restarts.push((
            run,
            Restart {
                engine,
                analyzing,
                low_priority,
                view: carried,
            },
        ));
        self.stop_run(run, effects);
    }

    // ---- the log ----

    fn analysis_open_log(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if self.prefs.analysis.log_lines == 0 {
            return;
        }
        let Some(view) = self.analysis.get_mut(run) else {
            return;
        };
        view.log_open = true;
        view.log_in_flight = true;
        let since = view.log_next;
        effects.push(Effect::ReadEngineLog {
            run,
            since,
            hub: self.engine_hub.clone(),
        });
    }

    fn analysis_log_tick(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let Some(view) = self.analysis.get_mut(run) else {
            return;
        };
        // One read at a time: a slow one must not pile up.
        if !view.log_open || view.log_in_flight {
            return;
        }
        view.log_in_flight = true;
        let since = view.log_next;
        effects.push(Effect::ReadEngineLog {
            run,
            since,
            hub: self.engine_hub.clone(),
        });
    }

    fn analysis_log_read(
        &mut self,
        run: RunId,
        since: u64,
        entries: Vec<mychess_engine::LogEntry>,
        next: u64,
    ) {
        let capacity = usize::try_from(self.prefs.analysis.log_lines)
            .unwrap_or(usize::MAX)
            .max(1);
        let Some(view) = self.analysis.get_mut(run) else {
            return;
        };
        view.log_in_flight = false;
        // An answer to a read made before the log was closed or read again.
        if !view.log_open || since != view.log_next {
            return;
        }
        view.log.extend(entries);
        if view.log.len() > capacity {
            let excess = view.log.len() - capacity;
            view.log.drain(..excess);
        }
        view.log_next = next;
    }

    // ---- adding to the game ----

    /// Sends a game message the way [`update`](super::update) would.
    pub(crate) fn apply_game(&mut self, message: GameMessage, effects: &mut Vec<Effect>) {
        // The line of the opening trainer is not edited from outside while
        // the user is to move (spec M8-5, D2258).
        if self.opening_editor_locked()
            || self.calvar_editor_locked()
            || self.modal_open()
            || self.refuse_edit()
            || self.refuse_shootout_edit()
        {
            return;
        }
        effects.extend(self.session.update(message));
        self.take_session_analysis_notice();
    }

    /// Whether the lines of `run` are about the position at the cursor
    /// (not a stale search, not another position).
    pub(super) fn lines_follow_cursor(&mut self, run: RunId) -> bool {
        let Some(target) = self.analysis_target() else {
            return false;
        };
        self.analysis.get(run).is_some_and(|v| v.is_about(&target))
    }

    /// Whether the lines of `run` may feed an edit of the game: a live
    /// execution that is analysing, with lines about the cursor (rule 13).
    pub(super) fn lines_usable(&mut self, run: RunId) -> bool {
        let analyzing = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running && r.analyzing);
        analyzing && self.lines_follow_cursor(run)
    }

    fn engine_display_name(&self, run: RunId) -> String {
        self.engine_runs
            .get(run)
            .and_then(|r| r.identity.as_ref().map(|i| i.name.clone()))
            .unwrap_or_else(|| self.engine_name(run))
    }

    fn analysis_comment(&self, run: RunId, line: &PvLine, wdl: bool) -> String {
        format_comment(
            &self.prefs.analysis,
            &self.engine_display_name(run),
            line.depth,
            line.score,
            if wdl { line.wdl } else { None },
        )
    }

    fn analysis_add_move(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if !self.edit_allowed(run) {
            return;
        }
        if !self.lines_usable(run) {
            return;
        }
        let first = self
            .analysis
            .get(run)
            .and_then(|v| v.lines.first())
            .filter(|l| l.multipv == 1)
            .and_then(|l| l.pv.first().copied());
        // A stale or empty line: ignored, with no error (the Scid does the
        // same).
        let Some(mv) = first.filter(|mv| self.session.position().is_legal(*mv)) else {
            return;
        };
        // "Already the next move" only advances; otherwise the session
        // decides (and may ask) as for a move played on the board.
        self.apply_game(
            GameMessage::PlayMove {
                mv,
                options: PlayOptions::default(),
            },
            effects,
        );
    }

    fn analysis_add_score(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        if !self.edit_allowed(run) || !self.lines_usable(run) {
            return;
        }
        let Some(line) = self
            .analysis
            .get(run)
            .and_then(|v| v.lines.first())
            .cloned()
        else {
            return;
        };
        // The Scid does not add the win/draw/loss to a lone score.
        let text = self.analysis_comment(run, &line, false);
        let node = self.session.cursor();
        self.apply_game(GameMessage::PrependComment { node, text }, effects);
    }

    fn analysis_add_lines(&mut self, run: RunId, pick: &Pick, effects: &mut Vec<Effect>) {
        if !self.edit_allowed(run) || !self.lines_usable(run) {
            return;
        }
        let Some(view) = self.analysis.get(run) else {
            return;
        };
        let chosen: Vec<&PvLine> = match pick {
            Pick::Number(n) => view.lines.iter().filter(|l| l.multipv == *n).collect(),
            Pick::All(AllLinesMode::SecondOnly) => {
                view.lines.iter().filter(|l| l.multipv == 2).collect()
            }
            Pick::All(_) => view.lines.iter().collect(),
        };
        let first_moves_only = matches!(pick, Pick::All(AllLinesMode::FirstMovesOnly));
        let lines: Vec<EngineLineToAdd> = chosen
            .into_iter()
            .map(|line| {
                let pv = if first_moves_only {
                    line.pv.iter().take(1).copied().collect()
                } else {
                    line.pv.clone()
                };
                EngineLineToAdd {
                    pv,
                    comment: Some(self.analysis_comment(run, line, true)),
                }
            })
            .collect();
        let node: NodeId = self.session.cursor();
        let message = match <[EngineLineToAdd; 1]>::try_from(lines) {
            Ok([EngineLineToAdd { pv, comment }]) => {
                GameMessage::AddEngineLine { node, pv, comment }
            }
            Err(lines) => GameMessage::AddEngineLines { node, lines },
        };
        self.apply_game(message, effects);
    }
}

fn file_name(path: &std::path::Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}
