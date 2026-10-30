//! The searches by position, material and moves (spec M6-1, slice b): the
//! forms, the start of a search, its answer, the first game of the filter
//! and the saved searches.
//!
//! Everything here is bookkeeping, which `update` may do. The scan of the
//! database is the effect [`Effect::AdvancedSearch`], run by `perform` on a
//! blocking thread over a copy of the filter of the slot (D3202, D3232).

use std::path::PathBuf;

use mychess_core::Position;
use mychess_db::{
    AdvancedQuery, BoardSearch, FILTER_LAST_MOVE, MaterialSearch, MoveToken, MovesSearch, Pattern,
    PieceRange, SavedAdvanced, SideRanges, material_presets, pattern_presets,
};

use super::AppState;
use crate::advanced_search::{
    AdvancedDone, AdvancedKind, AdvancedSearchMessage, AdvancedSearchNotice, AdvancedSearchPrefs,
    AdvancedSearchView, BishopsKind, BoardKind, CheckKind, MAX_MATERIAL_DIFF, MAX_PIECE_COUNT,
    MaterialPiece, PATTERN_ROWS, Running, SavedCommand, SavedFailure, SavedOutcome, SearchFailure,
    SearchOp, SearchSummary, SideKind,
};
use crate::advanced_search::{SAVED_QUEUE_MAX, SavedPending};
use crate::bases::{OpenControl, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::{AppNotice, BrowserLanding};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, Nav};
use crate::tree::TreeMessage;

/// The range of `piece` in `side`.
fn range_mut(side: &mut SideRanges, piece: MaterialPiece) -> &mut PieceRange {
    match piece {
        MaterialPiece::Queen => &mut side.queen,
        MaterialPiece::Rook => &mut side.rook,
        MaterialPiece::Bishop => &mut side.bishop,
        MaterialPiece::Knight => &mut side.knight,
        MaterialPiece::Minor => &mut side.minor,
        MaterialPiece::Pawn => &mut side.pawn,
    }
}

/// Puts every range of `material` inside `0..=MAX_PIECE_COUNT`, smaller end
/// first (a preset or a loaded search may hold anything).
fn tidy_ranges(material: &mut MaterialSearch) {
    for side in [&mut material.white, &mut material.black] {
        for piece in MaterialPiece::ALL {
            let range = range_mut(side, piece);
            let (a, b) = (
                range.min.min(MAX_PIECE_COUNT),
                range.max.min(MAX_PIECE_COUNT),
            );
            *range = PieceRange::new(a.min(b), a.max(b));
        }
    }
}

/// `patterns` with at least [`PATTERN_ROWS`] rows (empty ones at the end).
fn pad_patterns(patterns: &mut Vec<Pattern>) {
    if patterns.len() < PATTERN_ROWS {
        patterns.resize(PATTERN_ROWS, Pattern::default());
    }
}

impl AppState {
    /// The state of the window of the advanced searches.
    pub fn advanced_search(&self) -> &crate::advanced_search::AdvancedUi {
        &self.advanced
    }

    /// What the window draws, if it is open.
    pub fn advanced_search_view(&self) -> Option<AdvancedSearchView> {
        if !self.advanced.open {
            return None;
        }
        Some(self.advanced.build_view(
            &self.prefs.advanced_search,
            &self.bases,
            self.session.position().fen(),
            self.advanced_saved_dir().is_some(),
        ))
    }

    /// Whether an advanced search is running.
    pub(crate) fn advanced_search_running(&self) -> bool {
        self.advanced.running.is_some()
    }

    /// Stops the search in flight (the program is ending).
    pub(crate) fn advanced_search_cancel(&self) {
        self.advanced.cancel();
    }

    fn advanced_notice(&mut self, notice: AdvancedSearchNotice) {
        self.advanced.issue = Some(notice.clone());
        self.notice = Some(AppNotice::AdvancedSearch(notice));
    }

    /// The folder of the saved searches: next to the preferences file.
    fn advanced_saved_dir(&self) -> Option<PathBuf> {
        self.flow
            .prefs_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|dir| dir.join("searches"))
    }

    /// Changes the persisted options of the searches.
    fn advanced_prefs(
        &mut self,
        change: impl FnOnce(&mut AdvancedSearchPrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut prefs = self.prefs.advanced_search;
        change(&mut prefs);
        let prefs = prefs.sanitized();
        if prefs != self.prefs.advanced_search {
            self.prefs_message(
                PrefsMessage::Set(PrefChange::AdvancedSearch(prefs)),
                effects,
            );
        }
    }

    /// Handles an [`AdvancedSearchMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    pub(crate) fn advanced_search_message(
        &mut self,
        message: AdvancedSearchMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            AdvancedSearchMessage::Open(kind) => {
                self.advanced.open = true;
                self.advanced.tab = kind;
                self.advanced.issue = None;
                if !self.advanced.saved_listed {
                    self.advanced_saved_command(SavedCommand::List, effects);
                }
            }
            AdvancedSearchMessage::Close => {
                self.advanced.open = false;
                // A search nobody watches any more is stopped (D3391); its
                // answer is dropped by the request.
                if let Some(old) = self.advanced.running.take() {
                    old.control.cancel();
                    self.advanced_notice(AdvancedSearchNotice::Interrupted);
                }
            }
            AdvancedSearchMessage::SelectTab(kind) => {
                self.advanced.tab = kind;
                self.advanced.issue = None;
            }
            AdvancedSearchMessage::SetOp(kind, op) => self.advanced_prefs(
                |p| match kind {
                    AdvancedKind::Position => p.board_op = op,
                    AdvancedKind::Material => p.material_op = op,
                    AdvancedKind::Moves => p.moves_op = op,
                },
                effects,
            ),
            AdvancedSearchMessage::SetReference(kind, slot) => {
                if slot.is_none_or(|id| self.bases.slot(id).is_some())
                    && let Some(reference) = self.advanced.references.get_mut(kind.index())
                {
                    *reference = slot;
                }
            }
            AdvancedSearchMessage::SetBoardKind(kind) => {
                self.advanced_prefs(|p| p.board_kind = kind, effects);
            }
            AdvancedSearchMessage::SetBoardVariations(on) => {
                self.advanced_prefs(|p| p.board_variations = on, effects);
            }
            AdvancedSearchMessage::SetBoardIgnoreColors(on) => {
                self.advanced_prefs(|p| p.board_ignore_colors = on, effects);
            }
            AdvancedSearchMessage::UseCurrentPosition => self.advanced.fen_mode = false,
            AdvancedSearchMessage::SetFen(text) => {
                self.advanced.fen = text;
                self.advanced.fen_mode = true;
            }
            AdvancedSearchMessage::SetRange {
                color,
                piece,
                min,
                max,
            } => {
                let side = match color {
                    mychess_core::Color::White => &mut self.advanced.material.white,
                    mychess_core::Color::Black => &mut self.advanced.material.black,
                };
                *range_mut(side, piece) =
                    PieceRange::new(min.min(MAX_PIECE_COUNT), max.min(MAX_PIECE_COUNT));
            }
            AdvancedSearchMessage::SetDiff(diff) => {
                self.advanced.material.diff = diff.map(|(a, b)| {
                    (
                        a.clamp(-MAX_MATERIAL_DIFF, MAX_MATERIAL_DIFF),
                        b.clamp(-MAX_MATERIAL_DIFF, MAX_MATERIAL_DIFF),
                    )
                });
            }
            AdvancedSearchMessage::SetBishops(kind) => {
                self.advanced_prefs(|p| p.material_bishops = kind, effects);
            }
            AdvancedSearchMessage::SetMoveRange(min, max) => self.advanced_prefs(
                |p| {
                    p.material_move_min = min;
                    p.material_move_max = max;
                },
                effects,
            ),
            AdvancedSearchMessage::SetMinPlies(n) => {
                self.advanced_prefs(|p| p.material_min_plies = n, effects);
            }
            AdvancedSearchMessage::SetEndOnly(on) => {
                self.advanced_prefs(|p| p.material_end_only = on, effects);
            }
            AdvancedSearchMessage::SetMaterialIgnoreColors(on) => {
                self.advanced_prefs(|p| p.material_ignore_colors = on, effects);
            }
            AdvancedSearchMessage::MaterialZero => {
                self.advanced.material.white = SideRanges::ZERO;
                self.advanced.material.black = SideRanges::ZERO;
            }
            AdvancedSearchMessage::MaterialAny => {
                self.advanced.material.white = SideRanges::ANY;
                self.advanced.material.black = SideRanges::ANY;
                self.advanced.material.diff = None;
            }
            AdvancedSearchMessage::MaterialCurrentBoard => {
                let board = MaterialSearch::from_position(self.session.position());
                self.advanced.material.white = board.white;
                self.advanced.material.black = board.black;
            }
            AdvancedSearchMessage::ApplyEnding(index) => {
                if let Some(preset) = material_presets().get(index) {
                    preset.apply(&mut self.advanced.material);
                    tidy_ranges(&mut self.advanced.material);
                    pad_patterns(&mut self.advanced.material.patterns);
                    let ignore = preset.ignore_colors;
                    self.advanced_prefs(|p| p.material_ignore_colors = ignore, effects);
                }
            }
            AdvancedSearchMessage::ApplyPatternPreset(index) => {
                if let Some(preset) = pattern_presets().get(index) {
                    preset.apply(&mut self.advanced.material);
                    tidy_ranges(&mut self.advanced.material);
                    pad_patterns(&mut self.advanced.material.patterns);
                }
            }
            AdvancedSearchMessage::ClearPatterns => {
                self.advanced.material.patterns = vec![Pattern::default(); PATTERN_ROWS];
            }
            AdvancedSearchMessage::SetPatternPiece { index, piece } => {
                if let Some(p) = self.advanced.material.patterns.get_mut(index) {
                    p.piece = piece;
                }
            }
            AdvancedSearchMessage::SetPatternFile { index, file } => {
                if let Some(p) = self.advanced.material.patterns.get_mut(index) {
                    p.file = file;
                }
            }
            AdvancedSearchMessage::SetPatternRank { index, rank } => {
                if let Some(p) = self.advanced.material.patterns.get_mut(index) {
                    p.rank = rank;
                }
            }
            AdvancedSearchMessage::SetPatternPresent { index, present } => {
                if let Some(p) = self.advanced.material.patterns.get_mut(index) {
                    p.present = present;
                }
            }
            AdvancedSearchMessage::SetMovesText(text) => self.advanced.moves_text = text,
            AdvancedSearchMessage::SetMovesSide(side) => {
                self.advanced_prefs(|p| p.moves_side = side, effects);
            }
            AdvancedSearchMessage::SetMovesCheck(check) => {
                self.advanced_prefs(|p| p.moves_check = check, effects);
            }
            AdvancedSearchMessage::SetMovesVariations(on) => {
                self.advanced_prefs(|p| p.moves_variations = on, effects);
            }
            AdvancedSearchMessage::Start => self.advanced_start(effects),
            AdvancedSearchMessage::Stop => self.advanced.cancel(),
            AdvancedSearchMessage::Progress => {
                if let Some(running) = &self.advanced.running {
                    self.advanced.progress = running.control.progress();
                }
            }
            AdvancedSearchMessage::Finished { request, result } => {
                self.advanced_finished(request, result, effects);
            }
            AdvancedSearchMessage::Cancelled { request } => {
                if self.advanced.running.as_ref().map(|r| r.request) == Some(request) {
                    self.advanced.running = None;
                    self.advanced_notice(AdvancedSearchNotice::Interrupted);
                }
            }
            AdvancedSearchMessage::RefreshSaved => {
                self.advanced_saved_command(SavedCommand::List, effects);
            }
            AdvancedSearchMessage::SaveAs(name) => self.advanced_save(&name, effects),
            AdvancedSearchMessage::LoadSaved(name) => {
                self.advanced_saved_command(SavedCommand::Load(name), effects);
            }
            AdvancedSearchMessage::DeleteSaved(name) => {
                self.advanced_saved_command(SavedCommand::Delete(name), effects);
            }
            AdvancedSearchMessage::ImportSso(path) => {
                self.advanced_saved_command_anywhere(SavedCommand::ImportSso(path), effects);
            }
            AdvancedSearchMessage::SavedDone { request, result } => {
                self.advanced_saved_done(request, result, effects);
            }
        }
    }

    // ---- building the query ----

    /// The search of a tab as the engine takes it, or why the form is not
    /// valid. Nothing is changed.
    fn advanced_query(
        &self,
        kind: AdvancedKind,
    ) -> Result<(AdvancedQuery, Option<AdvancedSearchNotice>), AdvancedSearchNotice> {
        let prefs = &self.prefs.advanced_search;
        match kind {
            AdvancedKind::Position => {
                let position = if self.advanced.fen_mode {
                    let text = self.advanced.fen.trim();
                    // At most the six fields of a FEN: the core ignores
                    // what comes after them (D3392).
                    if text.split_ascii_whitespace().count() > 6 {
                        return Err(AdvancedSearchNotice::InvalidFen);
                    }
                    Position::from_fen(text).map_err(|_| AdvancedSearchNotice::InvalidFen)?
                } else {
                    self.session.position().clone()
                };
                let mut query = BoardSearch::new(position);
                query.kind = prefs.board_kind.engine();
                query.in_variations = prefs.board_variations;
                query.ignore_colors = prefs.board_ignore_colors;
                query.op = prefs.board_op.engine();
                Ok((AdvancedQuery::Position(query), None))
            }
            AdvancedKind::Material => {
                let mut query = self.advanced.material.clone();
                query.bishops = prefs.material_bishops.engine();
                query.moves = (prefs.material_move_min, prefs.material_move_max);
                query.min_plies = prefs.material_min_plies;
                query.end_only = prefs.material_end_only;
                query.ignore_colors = prefs.material_ignore_colors;
                query.op = prefs.material_op.engine();
                query
                    .validate()
                    .map_err(|e| AdvancedSearchNotice::from(&e))?;
                Ok((AdvancedQuery::Material(query), None))
            }
            AdvancedKind::Moves => {
                let query = MovesSearch::parse(
                    &self.advanced.moves_text,
                    prefs.moves_side.engine(),
                    prefs.moves_check.engine(),
                    prefs.moves_op.engine(),
                )
                .map_err(|e| AdvancedSearchNotice::from(&e))?;
                let mut query = query;
                query.in_variations = prefs.moves_variations;
                query
                    .validate()
                    .map_err(|e| AdvancedSearchNotice::from(&e))?;
                // A word with `+` or `#` raised the check test (D3226): say
                // so, but search.
                let raised = (query.check_test > prefs.moves_check.engine()).then(|| {
                    AdvancedSearchNotice::CheckTestRaised(CheckKind::from_engine(query.check_test))
                });
                Ok((AdvancedQuery::Moves(query), raised))
            }
        }
    }

    // ---- running ----

    fn advanced_start(&mut self, effects: &mut Vec<Effect>) {
        let kind = self.advanced.tab;
        // Everything that can refuse the search is checked before the one
        // in flight is touched: a form error leaves it running (D3389). The
        // search itself may be replaced, so it is not a reason to be busy.
        let old = self.advanced.running.take();
        let busy = self.list_busy();
        if busy {
            self.advanced.running = old;
            return;
        }
        let id = self
            .advanced
            .reference(kind)
            .unwrap_or_else(|| self.bases.current());
        let (query, raised) = match self.advanced_query(kind) {
            Ok(built) => built,
            Err(notice) => {
                self.advanced.running = old;
                return self.advanced_notice(notice);
            }
        };
        let Some(slot) = self.bases.slot(id).filter(|s| !s.needs_reload) else {
            self.advanced.running = old;
            return self.advanced_notice(AdvancedSearchNotice::NoDatabase);
        };
        // Starting a search stops the one in flight (D3232); its answer is
        // dropped by the request.
        if let Some(old) = old {
            old.control.cancel();
        }
        let (source, filter) = (slot.source.clone(), Box::new(slot.filter.clone()));
        let (len, count) = (slot.total(), slot.filtered());
        let request = self.new_request();
        let control = OpenControl::new();
        self.advanced.running = Some(Running {
            request,
            kind,
            slot: id,
            source: source.clone(),
            len,
            count,
            control: control.clone(),
        });
        self.advanced.progress = (0, 0);
        self.advanced.issue = None;
        if let Some(raised) = raised {
            self.advanced_notice(raised);
        }
        effects.push(Effect::AdvancedSearch {
            request,
            kind,
            slot: id,
            source,
            filter,
            query: Box::new(query),
            control,
        });
    }

    fn advanced_finished(
        &mut self,
        request: RequestId,
        result: Result<Box<AdvancedDone>, SearchFailure>,
        effects: &mut Vec<Effect>,
    ) {
        if self.advanced.running.as_ref().map(|r| r.request) != Some(request) {
            return;
        }
        let Some(running) = self.advanced.running.take() else {
            return;
        };
        let done = match result {
            Ok(done) => *done,
            Err(SearchFailure::Search(error)) => {
                return self.advanced_notice(AdvancedSearchNotice::from(&error));
            }
            Err(_) => return self.advanced_notice(AdvancedSearchNotice::Failed),
        };
        if done.report.interrupted {
            return self.advanced_notice(AdvancedSearchNotice::Interrupted);
        }
        // The slot must still hold the database and the filter the search
        // began with, or the answer describes something else.
        let current = self.bases.slot(running.slot).is_some_and(|slot| {
            slot.source == running.source
                && !slot.needs_reload
                && slot.total() == running.len
                && slot.filtered() == running.count
        });
        if !current || done.filter.len() != running.len {
            return self.advanced_notice(AdvancedSearchNotice::Stale);
        }
        let report = done.report;
        let Some(slot) = self.bases.slot_mut(running.slot) else {
            return;
        };
        slot.filter = done.filter;
        slot.filter_changed();
        slot.list.first = 1;
        // The value 255 stands for "last move" (`help.tcl` "mainline").
        let first = slot.filter.first().map(|n| {
            let ply = if slot.filter.get(n) == FILTER_LAST_MOVE {
                usize::MAX
            } else {
                usize::from(slot.filter.ply(n))
            };
            (n, ply)
        });
        self.advanced.summary = Some(SearchSummary {
            kind: running.kind,
            slot: running.slot,
            matched: report.matched,
            start_count: report.start_count,
            elapsed: report.elapsed,
            unreadable: report.unreadable_total,
        });
        self.advanced.issue = None;
        if report.unreadable_total > 0 {
            self.advanced_notice(AdvancedSearchNotice::Unreadable {
                count: report.unreadable_total,
            });
        }
        // The first game of the filter, at the position that matched (the
        // `loadFirstGame` of the Scid, D3233), and the tree stops following
        // the filter so that the plies of the search are not overwritten.
        if let Some((number, ply)) = first {
            self.advanced_load_first(running.slot, number, ply, effects);
        }
        self.tree_message(TreeMessage::SetAdjust(false), effects);
    }

    /// Loads game `number` of `id` at `ply` of its main line (the value of
    /// the filter minus one).
    pub(crate) fn advanced_load_first(
        &mut self,
        id: SlotId,
        number: u32,
        ply: usize,
        effects: &mut Vec<Effect>,
    ) {
        self.select_slot(id);
        if self.bases.current() != id {
            return;
        }
        let already = self
            .bases
            .slot(id)
            .is_some_and(|s| s.current_game == Some(number));
        if already {
            // The game is in the editor: only go to the position.
            let ply = u32::try_from(ply).unwrap_or(u32::MAX);
            drop(
                self.session
                    .update(GameMessage::Navigate(Nav::GoToMainlinePly(ply))),
            );
            return;
        }
        let landing = BrowserLanding {
            ply,
            close_browser: false,
            flipped: self.board_flipped,
            browser: RequestId(0),
        };
        self.request_base_game(id, number, Some(landing), effects);
    }

    // ---- saved searches ----

    fn advanced_saved_command(&mut self, command: SavedCommand, effects: &mut Vec<Effect>) {
        self.advanced_saved_command_anywhere(command, effects);
    }

    fn advanced_saved_command_anywhere(
        &mut self,
        command: SavedCommand,
        effects: &mut Vec<Effect>,
    ) {
        // Reading a `.sso` file needs no folder.
        let needs_dir = !matches!(command, SavedCommand::ImportSso(_));
        let dir = self.advanced_saved_dir();
        let dir = match (dir, needs_dir) {
            (Some(dir), _) => dir,
            (None, false) => PathBuf::new(),
            (None, true) => {
                // Listing with nowhere to look is not an error to shout
                // about: the window just has no list.
                if !matches!(command, SavedCommand::List) {
                    self.advanced_notice(AdvancedSearchNotice::NoSavedFolder);
                }
                return;
            }
        };
        if matches!(command, SavedCommand::List) {
            self.advanced.saved_listed = true;
        }
        // One operation at a time (D3390): the others wait their turn, so
        // no answer is dropped for a newer request.
        if self.advanced.saved_op.is_some() {
            let queue = &mut self.advanced.saved_queue;
            if matches!(command, SavedCommand::List)
                && queue.iter().any(|(_, c)| matches!(c, SavedCommand::List))
            {
                return; // a listing already waits
            }
            if queue.len() >= SAVED_QUEUE_MAX {
                self.advanced_notice(AdvancedSearchNotice::SavedFailed);
                return;
            }
            queue.push_back((dir, command));
            return;
        }
        self.advanced_saved_issue(dir, command, effects);
    }

    /// Sends one saved-search operation to `perform`.
    fn advanced_saved_issue(
        &mut self,
        dir: PathBuf,
        command: SavedCommand,
        effects: &mut Vec<Effect>,
    ) {
        let request = self.new_request();
        self.advanced.saved_op = Some(SavedPending {
            request,
            import: matches!(command, SavedCommand::ImportSso(_)),
            name: match &command {
                SavedCommand::Load(name) | SavedCommand::Delete(name) => Some(name.clone()),
                _ => None,
            },
        });
        effects.push(Effect::AdvancedSaved {
            request,
            dir,
            command,
        });
    }

    fn advanced_save(&mut self, name: &str, effects: &mut Vec<Effect>) {
        let query = match self.advanced_query(self.advanced.tab) {
            Ok((query, _)) => query,
            Err(notice) => return self.advanced_notice(notice),
        };
        match SavedAdvanced::new(name, query) {
            Ok(saved) => self.advanced_saved_command(SavedCommand::Save(Box::new(saved)), effects),
            Err(_) => self.advanced_notice(AdvancedSearchNotice::InvalidSavedName),
        }
    }

    fn advanced_saved_done(
        &mut self,
        request: RequestId,
        result: Result<SavedOutcome, SavedFailure>,
        effects: &mut Vec<Effect>,
    ) {
        if self.advanced.saved_op.as_ref().map(|op| op.request) != Some(request) {
            return;
        }
        let Some(op) = self.advanced.saved_op.take() else {
            return;
        };
        self.advanced_saved_outcome(op, result, effects);
        // The next operation waiting goes now (D3390).
        if self.advanced.saved_op.is_none()
            && let Some((dir, command)) = self.advanced.saved_queue.pop_front()
        {
            self.advanced_saved_issue(dir, command, effects);
        }
    }

    fn advanced_saved_outcome(
        &mut self,
        op: SavedPending,
        result: Result<SavedOutcome, SavedFailure>,
        effects: &mut Vec<Effect>,
    ) {
        let importing = op.import;
        let asked = op.name.unwrap_or_default();
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(SavedFailure::NotFound) => {
                return self.advanced_notice(AdvancedSearchNotice::SavedNotFound(asked));
            }
            Err(SavedFailure::InvalidName) => {
                return self.advanced_notice(AdvancedSearchNotice::InvalidSavedName);
            }
            Err(failure) => {
                // An `.sso` that cannot be read is told apart from a saved
                // search that cannot be written.
                let import = importing && failure != SavedFailure::Internal;
                return self.advanced_notice(if import {
                    AdvancedSearchNotice::ImportFailed
                } else {
                    AdvancedSearchNotice::SavedFailed
                });
            }
        };
        match outcome {
            SavedOutcome::Listed { entries, .. } => self.advanced.saved = entries,
            SavedOutcome::Saved(name) => {
                self.advanced_notice(AdvancedSearchNotice::Saved(name));
                self.advanced_saved_command(SavedCommand::List, effects);
            }
            SavedOutcome::Deleted(name, found) => {
                self.advanced_notice(if found {
                    AdvancedSearchNotice::Deleted(name)
                } else {
                    AdvancedSearchNotice::SavedNotFound(name)
                });
                self.advanced_saved_command(SavedCommand::List, effects);
            }
            SavedOutcome::Loaded(query) => {
                self.advanced.issue = None;
                self.advanced_apply_query(*query, effects);
            }
            SavedOutcome::Imported { search, warnings } => {
                self.advanced_apply_query(AdvancedQuery::Material(*search), effects);
                self.advanced_notice(AdvancedSearchNotice::Imported { warnings });
            }
        }
    }

    /// Puts a loaded search into its form and shows its tab.
    fn advanced_apply_query(&mut self, query: AdvancedQuery, effects: &mut Vec<Effect>) {
        match query {
            AdvancedQuery::Position(q) => {
                self.advanced.fen = q.position.fen();
                self.advanced.fen_mode = true;
                self.advanced.tab = AdvancedKind::Position;
                self.advanced_prefs(
                    |p| {
                        p.board_op = SearchOp::from_engine(q.op);
                        p.board_kind = BoardKind::from_engine(q.kind);
                        p.board_variations = q.in_variations;
                        p.board_ignore_colors = q.ignore_colors;
                    },
                    effects,
                );
            }
            AdvancedQuery::Material(q) => {
                let mut material = q.clone();
                tidy_ranges(&mut material);
                pad_patterns(&mut material.patterns);
                self.advanced.material = material;
                self.advanced.tab = AdvancedKind::Material;
                self.advanced_prefs(
                    |p| {
                        p.material_op = SearchOp::from_engine(q.op);
                        p.material_ignore_colors = q.ignore_colors;
                        p.material_end_only = q.end_only;
                        p.material_bishops = BishopsKind::from_engine(q.bishops);
                        p.material_move_min = q.moves.0;
                        p.material_move_max = q.moves.1;
                        p.material_min_plies = q.min_plies;
                    },
                    effects,
                );
            }
            AdvancedQuery::Moves(q) => {
                self.advanced.moves_text = q
                    .tokens
                    .iter()
                    .map(|token| match token {
                        MoveToken::San(text) => text.as_str(),
                        MoveToken::Any => "?",
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                self.advanced.tab = AdvancedKind::Moves;
                self.advanced_prefs(
                    |p| {
                        p.moves_op = SearchOp::from_engine(q.op);
                        p.moves_side = SideKind::from_engine(q.side);
                        p.moves_check = CheckKind::from_engine(q.check_test);
                        p.moves_variations = q.in_variations;
                    },
                    effects,
                );
            }
            _ => {}
        }
    }
}
