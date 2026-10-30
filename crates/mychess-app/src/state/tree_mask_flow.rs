//! The tree masks in the state (spec M5-4b, D1860 to D1879).
//!
//! One mask is open at a time ([`MaskSession`]). Its data is an
//! `Arc<TreeMask>` edited with `Arc::make_mut`: a job that holds a copy (a
//! save, a search) never sees an edit, and the state never waits for a job.
//! Reading, writing, filling from the games of a filter, searching and
//! building the outline are [`Effect`]s; the edits of one position, and the
//! filling from the line or the game, are quick and run in `update`.
//!
//! What the mask says about the table of the tree is computed once per
//! change of the mask, of the table or of the position (`mask_refresh`,
//! called at the end of every `update`), so [`AppState::tree_view`] only
//! copies a few small cells.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_core::{Game, NodeId, Position, Variant};
use mychess_db::tree::mask::{
    FillError, FillSide, MaskKey, MaskOp, MaskQuery, OutlineBudget, TreeMask, feed_game, feed_line,
    mask_san,
};

use super::AppState;
use crate::bases::OpenControl;
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::{AppNotice, DiscardChoice, Replacement};
use crate::host::{PickKind, PickSpec};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::tree::{
    Asked, CachedOnly, MaskBusy, MaskCell, MaskEdit, MaskEffect, MaskFillAnswer, MaskFillJob,
    MaskIssue, MaskJobKind, MaskLoaded, MaskMessage, MaskNotice, MaskOnlyRow, MaskOutlineState,
    MaskPending, MaskPrefs, MaskPrompt, MaskScope, MaskSearchState, MaskSession, MaskTarget,
    MaskView, PendingRead, PendingWrite, PickPurpose, RunningFill, SharedMask, TreeView, ViewCache,
    preview,
};

/// The most nodes of a game that "fill with the line / the game" accepts in
/// `update` (D1879).
const MAX_FILL_NODES: usize = 100_000;

/// How long the quit waits for a job of the mask (D1874).
const QUIT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The extension of the mask files.
const EXTENSION: &str = "stm";

/// Adds `.stm` to a name that has no extension (D1869).
fn with_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension(EXTENSION)
    }
}

impl AppState {
    // ---- what the front-end reads ----

    /// Whether a mask is open.
    pub fn mask_open(&self) -> bool {
        self.tree.mask.session.is_some()
    }

    /// The question "save the mask?" that waits for
    /// [`MaskMessage::Answer`]. The front-end shows it as a modal layer.
    pub fn mask_prompt(&self) -> Option<&MaskPrompt> {
        self.tree.mask.prompt.as_ref()
    }

    /// The last search of the mask, with its result when it has come back.
    pub fn mask_search(&self) -> Option<&MaskSearchState> {
        self.tree.mask.search.as_ref()
    }

    /// The outline of the mask, when asked for.
    pub fn mask_outline(&self) -> Option<&MaskOutlineState> {
        self.tree.mask.outline.as_ref()
    }

    /// The job of the mask that runs, with its progress (a fill reports
    /// the games done). The front-end polls it while it is `Some`.
    pub fn mask_busy(&self) -> Option<MaskBusy> {
        let ui = &self.tree.mask;
        if let Some(fill) = &ui.fill {
            let (done, total) = fill.control.progress();
            return Some(MaskBusy {
                kind: MaskJobKind::Fill,
                done,
                total,
                cancelling: fill.cancelling,
            });
        }
        let kind = if ui.reading.is_some() {
            MaskJobKind::Open
        } else if ui.writing.is_some() {
            MaskJobKind::Save
        } else if ui.searching.is_some() {
            MaskJobKind::Search
        } else if ui.outlining.is_some() {
            MaskJobKind::Outline
        } else {
            return None;
        };
        Some(MaskBusy {
            kind,
            done: 0,
            total: 0,
            cancelling: false,
        })
    }

    /// Every legal move of the position of the cursor in the text a mask
    /// stores (full SAN, `+` for check and mate: D1806), in the order of the
    /// "add this move to the mask" submenu (D1830). Empty without a mask,
    /// outside standard chess and during an online game.
    pub fn mask_legal_moves(&self) -> Vec<String> {
        if !self.mask_open() || self.assist_locked() || !self.mask_standard() {
            return Vec::new();
        }
        let position = self.session.position();
        position
            .legal_moves()
            .into_iter()
            .filter_map(|mv| mask_san(position, mv))
            .collect()
    }

    /// The whole comment of a move of the table (`Some(target)`) or of the
    /// position of the cursor (`None`): the text of a tool tip or the start
    /// of the editor (D1811). `None` when there is no mask or no such move.
    pub fn mask_comment(&self, target: Option<&MaskTarget>) -> Option<String> {
        if self.assist_locked() {
            return None;
        }
        let session = self.tree.mask.session.as_ref()?;
        let position = self.session.position();
        let entry = session.mask.mask().position(position);
        let Some(target) = target else {
            return Some(entry.map(|e| e.comment.clone()).unwrap_or_default());
        };
        let san = self.mask_target_san(target)?;
        Some(
            session
                .mask
                .mask()
                .lookup_move(position, &san)
                .map(|m| m.comment.clone())
                .unwrap_or_default(),
        )
    }

    fn mask_standard(&self) -> bool {
        self.session.game().variant() == Variant::Standard
    }

    // ---- the view ----

    /// Adds what the mask says to the snapshot of the tree (D1827, D1828).
    pub(super) fn mask_decorate(&self, view: &mut TreeView) {
        let ui = &self.tree.mask;
        let Some(session) = &ui.session else {
            return;
        };
        let prefs = &self.prefs.mask;
        let cache = self.mask_valid_cache();
        let interactive = cache.is_some_and(|c| c.fen == self.session.position().fen())
            && !view.searching
            && self.tree.autorefresh;
        if let Some(cache) = cache {
            for (row, cell) in view.rows.iter_mut().zip(&cache.cells) {
                row.mask.clone_from(cell);
            }
        }
        let (position_comment, position_comment_more) = cache
            .map(|c| c.position_comment.clone())
            .unwrap_or_default();
        let only_rows = cache
            .map(|c| {
                c.only
                    .iter()
                    .enumerate()
                    .map(|(index, only)| MaskOnlyRow {
                        index,
                        san: only.san.clone(),
                        cell: only.cell.clone(),
                        legal: only.mv.is_some(),
                        clickable: only.mv.is_some() && interactive,
                    })
                    .collect()
            })
            .unwrap_or_default();
        view.mask = Some(MaskView {
            file_name: session.name(),
            path: session.path.clone(),
            dirty: session.dirty,
            positions: session.mask.mask().len(),
            report: Arc::clone(&session.report),
            show_markers: prefs.show_markers,
            tooltips: prefs.tooltips,
            auto_load: prefs.auto_load,
            recent: prefs.recent.clone(),
            enabled: self.mask_standard(),
            position_comment,
            position_comment_more,
            only_rows,
            busy: self.mask_busy(),
        });
    }

    /// The cache, if it was made for the mask and the table on screen.
    fn mask_valid_cache(&self) -> Option<&ViewCache> {
        let session = self.tree.mask.session.as_ref()?;
        let cache = self.tree.mask.cache.as_ref()?;
        let shown = self.tree.shown.as_ref()?;
        (cache.rev == session.rev
            && cache.rows_id == Arc::as_ptr(&shown.result).addr()
            && cache.fen == shown.fen)
            .then_some(cache)
    }

    /// Brings what the mask says about the table up to date and starts the
    /// auto-load. Called at the end of every `update` (D1864).
    pub(super) fn mask_refresh(&mut self, effects: &mut Vec<Effect>) {
        self.mask_autoload(effects);
        self.mask_rebuild_cache();
    }

    fn mask_rebuild_cache(&mut self) {
        let wanted = self.mask_cache_wanted();
        if !wanted {
            self.tree.mask.cache = None;
            return;
        }
        if self.mask_valid_cache().is_some() {
            return;
        }
        let (Some(session), Some(shown)) =
            (self.tree.mask.session.as_ref(), self.tree.shown.as_ref())
        else {
            return;
        };
        let Ok(position) = Position::from_fen(&shown.fen) else {
            self.tree.mask.cache = None;
            return;
        };
        let rows = &shown.result.rows;
        // The `[end]` row and the rows the training hides have no move to
        // look up (and the training must not learn moves from the mask).
        let row_sans: Vec<Option<String>> = rows
            .iter()
            .map(|row| (!row.is_end() && !row.hidden).then(|| row.san.clone()))
            .collect();
        let asked: Vec<&str> = row_sans.iter().flatten().map(String::as_str).collect();
        let overlay = session.mask.mask().overlay(&position, &asked);
        let mut found = overlay.rows.iter();
        let cells = row_sans
            .iter()
            .map(|san| {
                san.as_ref().map(|_| {
                    found.next().copied().flatten().map_or_else(
                        || MaskCell {
                            in_mask: false,
                            nag: None,
                            color: mychess_db::tree::mask::MaskColor::White,
                            markers: [None, None],
                            comment: String::new(),
                            comment_more: false,
                        },
                        MaskCell::of,
                    )
                })
            })
            .collect();
        let only = if self.tree.training {
            Vec::new()
        } else {
            overlay
                .only_mask
                .iter()
                .map(|o| CachedOnly {
                    san: o.entry.san.clone(),
                    cell: MaskCell::of(o.entry),
                    mv: o.legal,
                })
                .collect()
        };
        let cache = ViewCache {
            rev: session.rev,
            rows_id: Arc::as_ptr(&shown.result).addr(),
            fen: shown.fen.clone(),
            row_sans,
            cells,
            only,
            position_comment: preview(overlay.comment, mychess_db::tree::mask::MAX_PREVIEW_BYTES),
        };
        self.tree.mask.cache = Some(cache);
    }

    /// Whether a table is on screen that the mask may annotate: the panel is
    /// open and has a table (or an empty answer), and no online game hides
    /// the help (D1860).
    fn mask_cache_wanted(&self) -> bool {
        self.tree.mask.session.is_some()
            && self.tree.open
            && !self.tree.unavailable
            && self.tree.issue.is_none()
            && self.tree.shown.is_some()
            && !self.assist_locked()
            && self.mask_standard()
    }

    // ---- the messages ----

    pub(super) fn mask_message(&mut self, message: MaskMessage, effects: &mut Vec<Effect>) {
        match message {
            MaskMessage::New => self.mask_request(MaskPending::New, effects),
            MaskMessage::Open(None) => {
                if !self.mask_job_running() {
                    self.mask_ask_file(PickPurpose::Open, effects);
                }
            }
            MaskMessage::Open(Some(path)) => self.mask_request(MaskPending::Open(path), effects),
            MaskMessage::Save => self.mask_save(effects),
            MaskMessage::SaveAs => {
                if self.mask_open() {
                    self.mask_ask_file(PickPurpose::Save(None), effects);
                } else {
                    self.notice = Some(AppNotice::Mask(MaskNotice::NoMask));
                }
            }
            MaskMessage::Close => self.mask_request(MaskPending::Close, effects),
            MaskMessage::Answer(choice) => self.mask_answer(choice, effects),
            MaskMessage::Edit {
                target,
                edit,
                scope,
            } => self.mask_edit(&target, &edit, scope),
            MaskMessage::SetPositionComment(text) => self.mask_set_position_comment(text),
            MaskMessage::PlayOnlyRow(index) => self.mask_play_only(index, effects),
            MaskMessage::FillLine(side) => self.mask_fill_line(side),
            MaskMessage::FillGame => self.mask_fill_game(),
            MaskMessage::FillFilter(side) => self.mask_fill_filter(side, effects),
            MaskMessage::CancelFill => {
                if let Some(fill) = self.tree.mask.fill.as_mut() {
                    fill.control.cancel();
                    fill.cancelling = true;
                }
            }
            MaskMessage::Search(query) => self.mask_search_start(*query, effects),
            MaskMessage::CloseSearch => {
                self.tree.mask.search = None;
                self.tree.mask.searching = None;
            }
            MaskMessage::BuildOutline => self.mask_outline_start(effects),
            MaskMessage::CloseOutline => {
                self.tree.mask.outline = None;
                self.tree.mask.outlining = None;
            }
            MaskMessage::GoTo(key) => self.mask_go_to(&key, effects),
            MaskMessage::SetShowMarkers(on) => self.mask_prefs(effects, |p| p.show_markers = on),
            MaskMessage::SetTooltips(on) => self.mask_prefs(effects, |p| p.tooltips = on),
            MaskMessage::SetAutoLoad(on) => self.mask_prefs(effects, |p| p.auto_load = on),
            MaskMessage::RemoveRecent(path) => {
                self.mask_prefs(effects, |p| {
                    p.remove_recent(&path);
                });
            }
            MaskMessage::Opened { request, result } => self.mask_opened(request, *result, effects),
            MaskMessage::Saved { request, result } => self.mask_saved(request, result, effects),
            MaskMessage::FillFinished(answer) => self.mask_fill_finished(*answer, effects),
            MaskMessage::Searched { request, hits } => {
                let current = self.tree.mask.session.as_ref().map(|s| s.rev);
                if let Some(asked) = self.tree.mask.searching.take_if(|a| a.request == request)
                    && let Some(search) = self.tree.mask.search.as_mut()
                {
                    search.hits = Some(Arc::new(*hits));
                    search.stale = current != Some(asked.rev);
                }
            }
            MaskMessage::Outlined { request, outline } => {
                let current = self.tree.mask.session.as_ref().map(|s| s.rev);
                if let Some(asked) = self.tree.mask.outlining.take_if(|a| a.request == request)
                    && let Some(state) = self.tree.mask.outline.as_mut()
                {
                    state.outline = Some(Arc::new(*outline));
                    state.stale = current != Some(asked.rev);
                }
            }
            MaskMessage::Deadline(generation) => self.mask_deadline(generation, effects),
        }
    }

    fn mask_prefs(&mut self, effects: &mut Vec<Effect>, change: impl FnOnce(&mut MaskPrefs)) {
        let mut prefs = (*self.prefs.mask).clone();
        change(&mut prefs);
        if prefs != *self.prefs.mask {
            self.prefs_message(
                PrefsMessage::Set(PrefChange::TreeMask(Box::new(prefs))),
                effects,
            );
        }
    }

    /// A job that owns the mask or the file: a fill, a read or a write.
    /// The mask cannot be replaced or closed meanwhile (D1875).
    fn mask_job_running(&mut self) -> bool {
        let ui = &self.tree.mask;
        let busy = ui.fill.is_some() || ui.reading.is_some() || ui.writing.is_some();
        if busy {
            self.notice = Some(AppNotice::Mask(MaskNotice::Busy));
        }
        busy
    }

    /// Whether the mask may be edited now. Besides the jobs above, a search
    /// or an outline holds a copy of the mask (an edit would copy all of it
    /// inside `update`: D1863), a question about saving is open, and a quit
    /// that decided about the mask is on its way out (D1877).
    fn mask_edit_busy(&mut self) -> bool {
        let ui = &self.tree.mask;
        let busy = ui.searching.is_some()
            || ui.outlining.is_some()
            || ui.prompt.is_some()
            || ui.quit.decided;
        if busy {
            self.notice = Some(AppNotice::Mask(MaskNotice::Busy));
        }
        busy || self.mask_job_running()
    }

    /// An open mask that edits can reach: not busy, standard chess.
    fn mask_ready(&mut self) -> bool {
        if self.tree.mask.session.is_none() {
            self.notice = Some(AppNotice::Mask(MaskNotice::NoMask));
            return false;
        }
        if self.mask_edit_busy() {
            return false;
        }
        if !self.mask_standard() {
            self.notice = Some(AppNotice::Mask(MaskNotice::NotStandard));
            return false;
        }
        true
    }

    fn fresh_rev(&mut self) -> u64 {
        self.tree.mask.next_rev += 1;
        self.tree.mask.next_rev
    }

    /// Records that the mask changed.
    fn mask_changed(&mut self) {
        let rev = self.fresh_rev();
        if let Some(session) = self.tree.mask.session.as_mut() {
            session.dirty = true;
            session.rev = rev;
        }
        // What was searched or outlined no longer shows the mask (D1878).
        if let Some(search) = self.tree.mask.search.as_mut() {
            search.stale = true;
        }
        if let Some(outline) = self.tree.mask.outline.as_mut() {
            outline.stale = true;
        }
    }

    // ---- editing ----

    /// The text of the move a target names.
    fn mask_target_san(&self, target: &MaskTarget) -> Option<String> {
        match target {
            MaskTarget::San(san) => Some(san.clone()),
            MaskTarget::Row(index) => {
                let cache = self.mask_valid_cache_for_cursor()?;
                cache.row_sans.get(*index)?.clone()
            }
            MaskTarget::MaskOnly(index) => {
                let cache = self.mask_valid_cache_for_cursor()?;
                cache.only.get(*index).map(|o| o.san.clone())
            }
        }
    }

    /// The cache, when its table is the position of the cursor (a row of a
    /// stale table is not edited).
    fn mask_valid_cache_for_cursor(&self) -> Option<&ViewCache> {
        self.mask_valid_cache()
            .filter(|c| c.fen == self.session.position().fen())
    }

    fn mask_edit(&mut self, target: &MaskTarget, edit: &MaskEdit, scope: MaskScope) {
        if !self.mask_ready() {
            return;
        }
        let Some(san) = self.mask_target_san(target) else {
            return;
        };
        let position = self.session.position().clone();
        let cursor = self.session.cursor();
        let op_of = |san: String| match edit {
            MaskEdit::Add => MaskOp::AddMove { san },
            MaskEdit::Remove => MaskOp::RemoveMove { san },
            MaskEdit::Comment(text) => MaskOp::SetMoveComment {
                san,
                text: text.clone(),
            },
            MaskEdit::Nag(nag) => MaskOp::SetNag {
                san,
                nag: nag.clone(),
            },
            MaskEdit::Color(color) => MaskOp::SetColor {
                san,
                color: color.clone(),
            },
            MaskEdit::Marker { slot, marker } => MaskOp::SetMarker {
                san,
                slot: *slot,
                marker: marker.clone(),
            },
        };
        let line = match scope {
            MaskScope::One => Vec::new(),
            MaskScope::Line => line_moves(
                self.session.game(),
                cursor,
                matches!(edit, MaskEdit::Remove),
            ),
        };
        let Some(session) = self.tree.mask.session.as_mut() else {
            return;
        };
        let mask: &mut TreeMask = Arc::make_mut(&mut session.mask.0);
        let mut changed = mask.apply(&position, &op_of(san));
        for (before, san) in line {
            changed |= mask.apply(&before, &op_of(san));
        }
        if changed {
            self.mask_changed();
        }
    }

    fn mask_set_position_comment(&mut self, text: String) {
        if !self.mask_ready() {
            return;
        }
        let position = self.session.position().clone();
        let Some(session) = self.tree.mask.session.as_mut() else {
            return;
        };
        let changed = Arc::make_mut(&mut session.mask.0)
            .apply(&position, &MaskOp::SetPositionComment { text });
        if changed {
            self.mask_changed();
        }
    }

    /// The click on a move only the mask has (D1828): the normal flow of a
    /// move of the user, like the click on a row of the table.
    fn mask_play_only(&mut self, index: usize, effects: &mut Vec<Effect>) {
        if self.tree.searching() || !self.tree.autorefresh {
            return;
        }
        let mv = self
            .mask_valid_cache_for_cursor()
            .and_then(|c| c.only.get(index))
            .and_then(|o| o.mv);
        if let Some(mv) = mv {
            effects.extend(super::update(
                self,
                crate::message::Message::Game(crate::session::GameMessage::PlayMove {
                    mv,
                    options: crate::session::PlayOptions::default(),
                }),
            ));
        }
    }

    // ---- filling ----

    fn mask_fill_line(&mut self, side: FillSide) {
        if !self.mask_ready() || self.mask_game_too_big() {
            return;
        }
        let cursor = self.session.cursor();
        let Some(session) = self.tree.mask.session.as_mut() else {
            return;
        };
        let result = feed_line(
            Arc::make_mut(&mut session.mask.0),
            self.session.game(),
            cursor,
            side,
        );
        self.mask_fill_result(&result);
    }

    /// The fill from the game runs inside `update`: a game with more nodes
    /// than [`MAX_FILL_NODES`] is refused (D1879).
    fn mask_game_too_big(&mut self) -> bool {
        let big = self.session.game().node_count() > MAX_FILL_NODES;
        if big {
            self.notice = Some(AppNotice::Mask(MaskNotice::GameTooLarge));
        }
        big
    }

    fn mask_fill_game(&mut self) {
        if !self.mask_ready() || self.mask_game_too_big() {
            return;
        }
        let Some(session) = self.tree.mask.session.as_mut() else {
            return;
        };
        let result = feed_game(
            Arc::make_mut(&mut session.mask.0),
            self.session.game(),
            FillSide::All,
        );
        self.mask_fill_result(&result);
    }

    fn mask_fill_result(&mut self, result: &Result<mychess_db::tree::mask::FillReport, FillError>) {
        match result {
            Ok(report) => {
                if report.changed {
                    self.mask_changed();
                }
                if report.mask_full {
                    self.notice = Some(AppNotice::Mask(MaskNotice::Full));
                } else if report.moves > 0 {
                    self.notice = Some(AppNotice::Mask(MaskNotice::Filled {
                        moves: report.moves,
                        games: report.games,
                        skipped: report.skipped,
                        cancelled: false,
                    }));
                }
            }
            Err(FillError::NotStandard) => {
                self.notice = Some(AppNotice::Mask(MaskNotice::NotStandard));
            }
            Err(_) => self.notice = Some(AppNotice::Mask(MaskNotice::CannotShowPosition)),
        }
    }

    fn mask_fill_filter(&mut self, side: FillSide, effects: &mut Vec<Effect>) {
        if !self.mask_ready() {
            return;
        }
        if self.tree.mask.pick.is_some() {
            self.notice = Some(AppNotice::Mask(MaskNotice::Busy));
            return;
        }
        let id = self.tree_target();
        let usable = self
            .bases
            .slot(id)
            .filter(|slot| {
                slot.total() > 0 && !slot.needs_reload && slot.filter.len() == slot.total()
            })
            .map(|slot| (slot.source.clone(), slot.filter.clone()));
        let (Some((source, filter)), Some(session)) = (usable, self.tree.mask.session.as_ref())
        else {
            self.notice = Some(AppNotice::Mask(MaskNotice::FillUnavailable));
            return;
        };
        let mask = session.mask.clone();
        self.tree.mask.next_job += 1;
        let job_id = self.tree.mask.next_job;
        let control = OpenControl::new();
        self.tree.mask.fill = Some(RunningFill {
            id: job_id,
            control: control.clone(),
            cancelling: false,
        });
        effects.push(Effect::Mask(MaskEffect::Fill {
            job: Box::new(MaskFillJob {
                id: job_id,
                mask,
                source,
                filter,
                side,
                control,
            }),
        }));
    }

    fn mask_fill_finished(&mut self, answer: MaskFillAnswer, effects: &mut Vec<Effect>) {
        let Some(_fill) = self.tree.mask.fill.take_if(|f| f.id == answer.id) else {
            return;
        };
        match answer.result {
            Ok(report) => {
                if report.changed
                    && let Some(session) = self.tree.mask.session.as_mut()
                {
                    session.mask = answer.mask;
                    self.mask_changed();
                }
                self.notice = Some(AppNotice::Mask(if report.mask_full {
                    MaskNotice::Full
                } else {
                    MaskNotice::Filled {
                        moves: report.moves,
                        games: report.games,
                        skipped: report.skipped,
                        cancelled: report.cancelled,
                    }
                }));
            }
            Err(_) => self.notice = Some(AppNotice::Mask(MaskNotice::FillUnavailable)),
        }
        self.mask_job_ended(effects);
    }

    // ---- search, outline, go to ----

    fn mask_search_start(&mut self, query: MaskQuery, effects: &mut Vec<Effect>) {
        let Some(session) = self.tree.mask.session.as_ref() else {
            self.notice = Some(AppNotice::Mask(MaskNotice::NoMask));
            return;
        };
        let (mask, rev) = (session.mask.clone(), session.rev);
        let request = self.new_request();
        self.tree.mask.searching = Some(Asked { request, rev });
        self.tree.mask.search = Some(MaskSearchState {
            query: query.clone(),
            hits: None,
            stale: false,
        });
        effects.push(Effect::Mask(MaskEffect::Search {
            request,
            mask,
            query: Box::new(query),
        }));
    }

    fn mask_outline_start(&mut self, effects: &mut Vec<Effect>) {
        let Some(session) = self.tree.mask.session.as_ref() else {
            self.notice = Some(AppNotice::Mask(MaskNotice::NoMask));
            return;
        };
        let (mask, rev) = (session.mask.clone(), session.rev);
        let position = Box::new(self.session.position().clone());
        let request = self.new_request();
        self.tree.mask.outlining = Some(Asked { request, rev });
        self.tree.mask.outline = Some(MaskOutlineState {
            outline: None,
            stale: false,
        });
        effects.push(Effect::Mask(MaskEffect::Outline {
            request,
            mask,
            position,
            budget: OutlineBudget::default(),
        }));
    }

    fn mask_go_to(&mut self, key: &MaskKey, effects: &mut Vec<Effect>) {
        let position = key
            .full_fen()
            .and_then(|fen| Position::from_fen(&fen).ok())
            .filter(|p| p.variant() == Variant::Standard);
        match position {
            Some(position) => self.begin_replace(Replacement::FromPosition(position), effects),
            None => self.notice = Some(AppNotice::Mask(MaskNotice::CannotShowPosition)),
        }
    }

    // ---- files ----

    /// Asks for the question about saving when the mask is altered, else
    /// does `then` (D1817).
    fn mask_request(&mut self, then: MaskPending, effects: &mut Vec<Effect>) {
        if self.mask_job_running() {
            return;
        }
        if self.tree.mask.quit.decided {
            // The quit already decided about the mask (D1877).
            self.notice = Some(AppNotice::Mask(MaskNotice::Busy));
            return;
        }
        self.mask_guard(then, effects);
    }

    fn mask_guard(&mut self, then: MaskPending, effects: &mut Vec<Effect>) {
        let dirty = self.tree.mask.session.as_ref().is_some_and(|s| s.dirty);
        if !dirty {
            self.mask_proceed(then, effects);
        } else if self.tree.mask.prompt.is_none() {
            let name = self.tree.mask.session.as_ref().and_then(MaskSession::name);
            self.tree.mask.prompt = Some(MaskPrompt { name, then });
        }
    }

    fn mask_proceed(&mut self, then: MaskPending, effects: &mut Vec<Effect>) {
        match then {
            MaskPending::New => {
                let rev = self.fresh_rev();
                self.mask_install(Some(MaskSession {
                    id: rev,
                    path: None,
                    mask: SharedMask::new(TreeMask::new()),
                    dirty: false,
                    report: Arc::default(),
                    rev,
                }));
            }
            MaskPending::Open(path) => self.mask_start_read(path, effects),
            MaskPending::Close => self.mask_install(None),
            MaskPending::Quit => {
                self.tree.mask.quit.decided = true;
                self.finish_quit(effects);
            }
        }
    }

    /// Replaces the open mask; what was derived from the old one goes.
    fn mask_install(&mut self, session: Option<MaskSession>) {
        let ui = &mut self.tree.mask;
        ui.session = session;
        ui.cache = None;
        ui.search = None;
        ui.searching = None;
        ui.outline = None;
        ui.outlining = None;
    }

    fn mask_start_read(&mut self, path: PathBuf, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        self.tree.mask.reading = Some(PendingRead {
            request,
            path: path.clone(),
        });
        effects.push(Effect::Mask(MaskEffect::Read { request, path }));
    }

    fn mask_opened(
        &mut self,
        request: RequestId,
        result: Result<MaskLoaded, MaskIssue>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(read) = self.tree.mask.reading.take_if(|r| r.request == request) else {
            return;
        };
        match result {
            Ok(loaded) => {
                let count = loaded.report.total_warnings();
                let rev = self.fresh_rev();
                self.mask_install(Some(MaskSession {
                    id: rev,
                    path: Some(read.path.clone()),
                    mask: loaded.mask,
                    dirty: false,
                    report: Arc::new(loaded.report),
                    rev,
                }));
                self.mask_remember(&read.path, effects);
                if count > 0 {
                    self.notice = Some(AppNotice::Mask(MaskNotice::Warnings { count }));
                }
            }
            Err(issue) => {
                self.notice = Some(AppNotice::Mask(MaskNotice::OpenFailed {
                    path: read.path,
                    issue,
                }));
            }
        }
        // A quit that waited for the read goes on (D1874).
        self.mask_job_ended(effects);
    }

    /// Puts a file first in the recent list (D1819).
    fn mask_remember(&mut self, path: &Path, effects: &mut Vec<Effect>) {
        self.mask_prefs(effects, |p| p.push_recent(path));
    }

    fn mask_save(&mut self, effects: &mut Vec<Effect>) {
        let Some(session) = self.tree.mask.session.as_ref() else {
            self.notice = Some(AppNotice::Mask(MaskNotice::NoMask));
            return;
        };
        match session.path.clone() {
            Some(path) => {
                self.mask_start_write(path, None, effects);
            }
            None => self.mask_ask_file(PickPurpose::Save(None), effects),
        }
    }

    /// Starts writing the mask; `false` when a write is already running.
    fn mask_start_write(
        &mut self,
        path: PathBuf,
        then: Option<MaskPending>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let Some(session) = self.tree.mask.session.as_ref() else {
            return false;
        };
        if self.tree.mask.writing.is_some() {
            self.notice = Some(AppNotice::Mask(MaskNotice::Busy));
            return false;
        }
        let (mask, rev, id) = (session.mask.clone(), session.rev, session.id);
        let request = self.new_request();
        self.tree.mask.writing = Some(PendingWrite {
            request,
            session: id,
            path: path.clone(),
            rev,
            then,
        });
        effects.push(Effect::Mask(MaskEffect::Write {
            request,
            path,
            mask,
        }));
        true
    }

    fn mask_saved(
        &mut self,
        request: RequestId,
        result: Result<u64, MaskIssue>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(write) = self.tree.mask.writing.take_if(|w| w.request == request) else {
            return;
        };
        match result {
            Ok(_) => {
                // The answer is for the mask the write was started for; if
                // another one was opened meanwhile, it keeps its own path
                // and state (D1875).
                let same = self
                    .tree
                    .mask
                    .session
                    .as_mut()
                    .filter(|s| s.id == write.session)
                    .map(|session| {
                        session.path = Some(write.path.clone());
                        // An edit made while the file was being written is
                        // not in it.
                        if session.rev == write.rev {
                            session.dirty = false;
                        }
                    })
                    .is_some();
                self.mask_remember(&write.path, effects);
                self.notice = Some(AppNotice::Mask(MaskNotice::Saved));
                match write.then {
                    Some(MaskPending::Quit) => {
                        // The wait (and its timer) is over (D1876).
                        self.tree.mask.quit.waiting = false;
                        self.tree.mask.quit.deadline_armed = false;
                        self.mask_quit_resume(effects);
                    }
                    Some(then) if same => self.mask_guard(then, effects),
                    _ => self.mask_job_ended(effects),
                }
            }
            Err(issue) => {
                self.notice = Some(AppNotice::Mask(MaskNotice::SaveFailed {
                    path: write.path,
                    issue,
                }));
                if let Some(then) = &write.then {
                    // What waited for the save does not happen (case 15):
                    // the quit is given up, with no wait and no timer left.
                    self.mask_quit_reset();
                    if *then == MaskPending::Quit {
                        self.flow.quit_asked.clear();
                        self.spelling_quit_undone(effects);
                    }
                }
                self.mask_job_ended(effects);
            }
        }
    }

    fn mask_ask_file(&mut self, purpose: PickPurpose, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        let (kind, file_name) = match &purpose {
            PickPurpose::Open => (PickKind::OpenMask, None),
            PickPurpose::Save(_) => (
                PickKind::SaveMask,
                Some(
                    self.tree
                        .mask
                        .session
                        .as_ref()
                        .and_then(MaskSession::name)
                        .unwrap_or_else(|| format!("mask.{EXTENSION}")),
                ),
            ),
        };
        let initial_dir = self
            .prefs
            .mask
            .last_dir
            .clone()
            .or_else(|| self.prefs.files.last_open_dir.clone());
        self.tree.mask.pick = Some((request, purpose));
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind,
            initial_dir,
            file_name,
        }));
    }

    /// The answer of the file dialog, when it was asked by the masks.
    /// Returns whether it was.
    pub(crate) fn mask_file_picked(
        &mut self,
        request: RequestId,
        path: Option<&Path>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let Some((_, purpose)) = self.tree.mask.pick.take_if(|(id, _)| *id == request) else {
            return false;
        };
        let Some(path) = path else {
            // The user gave up: what waited for the name does not happen.
            if matches!(purpose, PickPurpose::Save(Some(_))) {
                self.mask_quit_reset();
                self.flow.quit_asked.clear();
                if matches!(purpose, PickPurpose::Save(Some(MaskPending::Quit))) {
                    self.spelling_quit_undone(effects);
                }
            }
            return true;
        };
        match purpose {
            PickPurpose::Open => self.mask_request(MaskPending::Open(path.to_path_buf()), effects),
            PickPurpose::Save(then) => {
                self.mask_start_write(with_extension(path.to_path_buf()), then, effects);
            }
        }
        true
    }

    fn mask_answer(&mut self, choice: DiscardChoice, effects: &mut Vec<Effect>) {
        let Some(prompt) = self.tree.mask.prompt.take() else {
            return;
        };
        match choice {
            DiscardChoice::Cancel => {
                self.mask_quit_reset();
                self.flow.quit_asked.clear();
                if prompt.then == MaskPending::Quit {
                    self.spelling_quit_undone(effects);
                }
            }
            DiscardChoice::DontSave => self.mask_proceed(prompt.then, effects),
            DiscardChoice::Save => {
                let path = self.tree.mask.session.as_ref().and_then(|s| s.path.clone());
                match path {
                    Some(path) => {
                        if !self.mask_start_write(path, Some(prompt.then.clone()), effects) {
                            // A write is running: the question stays.
                            self.tree.mask.prompt = Some(prompt);
                        }
                    }
                    None => self.mask_ask_file(PickPurpose::Save(Some(prompt.then)), effects),
                }
            }
        }
    }

    // ---- auto-load ----

    fn mask_autoload(&mut self, effects: &mut Vec<Effect>) {
        if !self.tree.open {
            self.tree.mask.autoload_tried = false;
            return;
        }
        if self.tree.mask.autoload_tried || self.flow.prefs_loading {
            return;
        }
        self.tree.mask.autoload_tried = true;
        if !self.prefs.mask.auto_load
            || self.tree.mask.session.is_some()
            || self.tree.mask.reading.is_some()
        {
            return;
        }
        if let Some(path) = self.prefs.mask.recent.first().cloned() {
            self.mask_start_read(path, effects);
        }
    }

    // ---- the quit (D1873, D1874) ----

    /// Forgets what a quit decided; called when a quit starts and when it is
    /// given up.
    pub(crate) fn mask_quit_reset(&mut self) {
        let quit = &mut self.tree.mask.quit;
        quit.decided = false;
        quit.timed_out = false;
        // Nothing is waited for any more; a timer still out has an older
        // generation than the next one armed, so it is ignored (D1874).
        quit.waiting = false;
        quit.deadline_armed = false;
    }

    /// Called by the quit after the games were dealt with: cancels what the
    /// mask runs, waits for it a few seconds, then asks about an altered
    /// mask. Returns whether the quit must wait.
    pub(crate) fn mask_quit_gate(&mut self, effects: &mut Vec<Effect>) -> bool {
        let ui = &mut self.tree.mask;
        if ui.quit.decided {
            return false;
        }
        let busy = ui.fill.is_some() || ui.writing.is_some() || ui.reading.is_some();
        if busy && !ui.quit.timed_out {
            if let Some(fill) = ui.fill.as_mut() {
                fill.control.cancel();
                fill.cancelling = true;
            }
            // A save that was asked goes on with the quit once it ends.
            if let Some(write) = ui.writing.as_mut() {
                write.then = Some(MaskPending::Quit);
            }
            ui.quit.waiting = true;
            if !ui.quit.deadline_armed {
                ui.quit.deadline_armed = true;
                ui.quit.deadline_generation += 1;
                effects.push(Effect::Mask(MaskEffect::Deadline {
                    delay: QUIT_WAIT,
                    generation: ui.quit.deadline_generation,
                }));
            }
            return true;
        }
        if let Some(prompt) = ui.prompt.as_mut() {
            // The question open now (for instance, "close the mask?") is
            // the quit's question from here on.
            prompt.then = MaskPending::Quit;
            return true;
        }
        if ui.session.as_ref().is_some_and(|s| s.dirty) {
            let name = ui.session.as_ref().and_then(MaskSession::name);
            ui.prompt = Some(MaskPrompt {
                name,
                then: MaskPending::Quit,
            });
            return true;
        }
        ui.quit.decided = true;
        false
    }

    fn mask_quit_resume(&mut self, effects: &mut Vec<Effect>) {
        if !self.mask_quit_gate(effects) {
            self.finish_quit(effects);
        }
    }

    /// A job of the mask ended: a quit that waited for it goes on.
    fn mask_job_ended(&mut self, effects: &mut Vec<Effect>) {
        if std::mem::take(&mut self.tree.mask.quit.waiting) {
            self.tree.mask.quit.deadline_armed = false;
            self.mask_quit_resume(effects);
        }
    }

    fn mask_deadline(&mut self, generation: u64, effects: &mut Vec<Effect>) {
        let quit = &mut self.tree.mask.quit;
        if !quit.deadline_armed || generation != quit.deadline_generation || !quit.waiting {
            return;
        }
        // The jobs are given up: their late answers find no job and are
        // dropped.
        quit.deadline_armed = false;
        quit.waiting = false;
        quit.timed_out = true;
        let ui = &mut self.tree.mask;
        if let Some(fill) = ui.fill.take() {
            fill.control.cancel();
        }
        ui.writing = None;
        ui.reading = None;
        self.mask_quit_resume(effects);
    }
}

/// The moves of the line of the cursor an edit with the scope "line" also
/// reaches (D1822): each with the position it is played from and its text.
/// Backwards (every edit but "remove"): the moves that led to the cursor,
/// last first. Forwards ("remove"): the moves of the main line after the one
/// that follows the cursor, which the clicked move stands for.
fn line_moves(game: &Game, cursor: NodeId, forwards: bool) -> Vec<(Position, String)> {
    let mut out = Vec::new();
    if forwards {
        let first = |node: NodeId| game.children(node).ok().and_then(|c| c.first().copied());
        let mut node = first(cursor);
        while let Some(current) = node {
            let Some(next) = first(current) else {
                break;
            };
            if let (Ok(before), Ok(mv)) = (game.position_at(current), game.mv(next))
                && let Some(san) = mask_san(before, mv)
            {
                out.push((before.clone(), san));
            }
            node = Some(next);
        }
    } else if let Ok(path) = game.path_to(cursor) {
        for id in path.into_iter().rev() {
            let Ok(Some(parent)) = game.parent(id) else {
                continue;
            };
            if let (Ok(before), Ok(mv)) = (game.position_at(parent), game.mv(id))
                && let Some(san) = mask_san(before, mv)
            {
                out.push((before.clone(), san));
            }
        }
    }
    out
}
