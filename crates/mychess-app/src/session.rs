//! The game editing session: game, cursor, history and interaction state.
//!
//! [`GameSession`] is the model of the "edit a game" use case. Every message
//! goes through [`GameSession::update`], which is pure and synchronous. Every
//! change to the [`Game`] goes through one funnel ([`GameSession::edit`]) so
//! that undo points, the revision counter and the "altered" flag cannot be
//! forgotten.
//!
//! Spec: `docs/specs/m2-1-edicao-de-partida.md`.

mod analysis;
mod annotate;
mod autoplay;
mod edit;
mod entry;
mod history;
mod message;
mod nav;
mod notice;
mod play;
mod settings;
mod strip;

#[cfg(test)]
mod tests;

use std::collections::VecDeque;

use mychess_core::{Game, GameError, Move, NodeId, Position};

use crate::analysis::AnalysisNotice;
use crate::effect::Effect;

pub use analysis::EngineLineToAdd;
pub use annotate::{
    NAG_SHORTCUTS, NagCategory, NagFieldError, nag_category, nag_field_text, parse_nag_field,
    parse_nag_list,
};
pub use autoplay::{Autoplay, MAX_AUTOPLAY_DELAY, MIN_AUTOPLAY_DELAY};
pub use entry::{Candidate, MoveEntry};
pub use message::{GameMessage, Nav, PlayOptions, ReplaceChoice, VariationTarget};
pub use nav::{VariationChoice, VariationChooser};
pub use notice::Notice;
pub use play::{PendingMove, Prompt};
pub use settings::{EditSettings, MoveEntrySettings};
pub use strip::StripKind;

use history::Trial;

/// Number of undo levels kept (Scid: `UNDO_MAX`).
pub const UNDO_LIMIT: usize = 20;

/// Identifies one state of the game for the "altered" flag.
///
/// Every effective edit gets a fresh value that never repeats, so undoing
/// back to the saved state makes the game clean again, while a saved state
/// that fell off the undo stack can never be matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revision(u64);

/// Game, cursor and revision at one point in time.
#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    game: Game,
    cursor: NodeId,
    revision: Revision,
}

/// A game being edited. See the module documentation.
#[derive(Debug)]
pub struct GameSession {
    game: Game,
    cursor: NodeId,
    revision: Revision,
    revision_counter: u64,
    changes: u64,
    generation: u64,
    saved_revision: Option<Revision>,
    undo: VecDeque<Snapshot>,
    redo: Vec<Snapshot>,
    trial: Option<Trial>,
    prompt: Option<Prompt>,
    pending_variation: Option<NodeId>,
    chooser: Option<VariationChooser>,
    entry: MoveEntry,
    autoplay: Autoplay,
    notice: Option<Notice>,
    analysis_notice: Option<AnalysisNotice>,
    settings: EditSettings,
    /// An open edit group (spec M4-6, D469): `Some(pushed)`, where `pushed`
    /// tells whether the group already has its undo point.
    group: Option<bool>,
    /// A game marked as tactical exercises that no database holds yet
    /// (the `T` flag of the Scid, spec M4-6 D471).
    tactics_pending: bool,
    /// While on, a legal move that would be played is only recorded in
    /// `captured` and the game is left alone (the guided review of the game
    /// report judges the move instead of playing it, spec M4-10, D1580).
    capture_moves: bool,
    captured: Option<Move>,
}

impl Default for GameSession {
    fn default() -> GameSession {
        GameSession::new(Game::new())
    }
}

impl GameSession {
    /// A session editing `game`, cursor at the start, nothing altered.
    pub fn new(game: Game) -> GameSession {
        let settings = EditSettings::default();
        let cursor = game.root();
        let revision = Revision(0);
        GameSession {
            game,
            cursor,
            revision,
            revision_counter: 0,
            changes: 0,
            generation: 0,
            saved_revision: Some(revision),
            undo: VecDeque::new(),
            redo: Vec::new(),
            trial: None,
            prompt: None,
            pending_variation: None,
            chooser: None,
            entry: MoveEntry::default(),
            autoplay: Autoplay::new(settings.autoplay_delay),
            notice: None,
            analysis_notice: None,
            settings,
            group: None,
            tactics_pending: false,
            capture_moves: false,
            captured: None,
        }
    }

    /// Runs `message` with the moves captured instead of played, and returns
    /// the effects and the legal move the message resolved to, if any. Any
    /// remark or prompt of the message (illegal move, promotion question)
    /// is kept as usual.
    pub(crate) fn update_capturing(&mut self, message: GameMessage) -> (Vec<Effect>, Option<Move>) {
        self.capture_moves = true;
        self.captured = None;
        let effects = self.update(message);
        self.capture_moves = false;
        (effects, self.captured.take())
    }

    /// Replaces the game (new or loaded). Like [`GameSession::new`], but
    /// keeps the preferences; leaves trial mode, stops autoplay and clears
    /// the undo history.
    pub fn replace_game(&mut self, game: Game) {
        self.cursor = game.root();
        self.game = game;
        self.generation = self.generation.saturating_add(1);
        self.changes = self.changes.saturating_add(1);
        self.revision = self.fresh_revision();
        self.saved_revision = Some(self.revision);
        self.undo.clear();
        self.redo.clear();
        self.trial = None;
        self.prompt = None;
        self.reset_interaction();
        self.autoplay.running = false;
        self.autoplay.generation = self.autoplay.generation.wrapping_add(1);
        self.notice = None;
        self.analysis_notice = None;
        self.group = None;
        self.tactics_pending = false;
    }

    /// Applies one message and returns the effects to run.
    ///
    /// Pure: no I/O and no timers (those are [`Effect`]s).
    pub fn update(&mut self, message: GameMessage) -> Vec<Effect> {
        let mut effects = Vec::new();
        let is_tick = matches!(message, GameMessage::AutoplayTick { .. });
        // A tick must not erase a notice the user has not read yet.
        if !is_tick {
            self.notice = None;
            self.analysis_notice = None;
        }
        if self.prompt.is_some() && !message.allowed_with_prompt() {
            if !is_tick {
                self.notice = Some(Notice::PromptPending);
            }
            return effects;
        }
        match message {
            GameMessage::Navigate(nav) => self.navigate(nav),
            GameMessage::OpenVariationChooser => self.open_chooser(),
            GameMessage::ChooserMove(delta) => self.chooser_move(delta),
            GameMessage::ChooserConfirm(index) => self.chooser_confirm(index),
            GameMessage::CloseVariationChooser => self.chooser = None,
            GameMessage::ToggleAutoplay => self.toggle_autoplay(&mut effects),
            GameMessage::StartAutoplay => self.start_autoplay(&mut effects),
            GameMessage::StopAutoplay => self.stop_autoplay(),
            GameMessage::SetAutoplayDelay(delay) => self.set_autoplay_delay(delay),
            GameMessage::AutoplayTick { generation } => {
                self.autoplay_tick(generation, &mut effects);
            }
            GameMessage::PlayMove { mv, options } => self.play_move(mv, options),
            GameMessage::PlaySquares {
                from,
                to,
                promotion,
                options,
            } => self.play_squares(from, to, promotion, options),
            GameMessage::PlayNullMove => self.play_null_move(),
            GameMessage::StartVariation => self.start_variation(),
            GameMessage::AnswerReplace(choice) => self.answer_replace(choice),
            GameMessage::AnswerPromotion(role) => self.answer_promotion(role),
            GameMessage::DismissPrompt => self.dismiss_prompt(),
            GameMessage::RequestStrip { kind, confirm } => self.request_strip(kind, confirm),
            GameMessage::AnswerStrip(yes) => self.answer_strip(yes),
            GameMessage::MoveEntryChar(ch) => self.entry_char(ch),
            GameMessage::MoveEntryTyped(ch) => self.entry_typed(ch),
            GameMessage::MoveEntryBackspace => self.entry_backspace(),
            GameMessage::MoveEntryCommit => self.entry_commit(),
            GameMessage::MoveEntryClear => self.entry.clear(),
            GameMessage::EnterMoveText(text) => self.enter_move_text(&text),
            GameMessage::PromoteVariation(target) => self.promote_variation(target),
            GameMessage::MakeFirstVariation(target) => self.make_first_variation(target),
            GameMessage::DeleteVariation(target) => self.delete_variation(target),
            GameMessage::TruncateEnd => self.truncate_end(),
            GameMessage::TruncateFrom => self.truncate_from(),
            GameMessage::TruncateStart => self.truncate_start(),
            GameMessage::StripComments => self.strip_comments(),
            GameMessage::StripVariations => self.strip_variations(),
            GameMessage::StripNags => self.strip_nags(),
            GameMessage::SetComment { node, text } => self.set_comment(node, &text),
            GameMessage::SetCommentBefore { node, text } => self.set_comment_before(node, &text),
            GameMessage::AddNag(nag) => self.add_nag(nag),
            GameMessage::ToggleNagAt { node, nag } => self.toggle_nag_at(node, nag),
            GameMessage::ToggleNag(nag) => self.toggle_nag(nag),
            GameMessage::RemoveNag(nag) => self.remove_nag(nag),
            GameMessage::ClearNags => self.clear_nags(),
            GameMessage::SetNags(nags) => self.set_nags(&nags),
            GameMessage::Undo => self.undo(),
            GameMessage::Redo => self.redo(),
            GameMessage::EnterTrial { null_move } => self.enter_trial(null_move),
            GameMessage::ExitTrial => self.exit_trial(),
            GameMessage::ToggleTrial { null_move } => {
                if self.trial.is_some() {
                    self.exit_trial();
                } else {
                    self.enter_trial(null_move);
                }
            }
            GameMessage::ApplySettings(settings) => self.apply_settings(settings),
            GameMessage::AddEngineLine { node, pv, comment } => {
                self.add_engine_lines(node, vec![EngineLineToAdd { pv, comment }]);
            }
            GameMessage::AddEngineLines { node, lines } => self.add_engine_lines(node, lines),
            GameMessage::PrependComment { node, text } => self.prepend_comment(node, &text),
        }
        effects
    }

    // ---- reading ----

    /// The game being edited.
    pub fn game(&self) -> &Game {
        &self.game
    }

    /// The node whose position is shown (the root before any move).
    pub fn cursor(&self) -> NodeId {
        self.cursor
    }

    /// The position at the cursor.
    pub fn position(&self) -> &Position {
        self.game
            .position_at(self.cursor)
            .unwrap_or_else(|_| self.game.start_position())
    }

    /// Whether the game was altered since it was loaded or last saved.
    /// In trial mode this is the value from before the trial.
    pub fn is_dirty(&self) -> bool {
        let shown = self
            .trial
            .as_ref()
            .map_or(self.revision, |trial| trial.backup.revision);
        Some(shown) != self.saved_revision
    }

    /// Records that the game was saved (clears the altered flag). In trial
    /// mode the game that counts as saved is the one from before the trial,
    /// so a caller that writes the game to disk must leave trial mode first
    /// (the file would otherwise hold the trial moves).
    pub fn mark_saved(&mut self) {
        self.saved_revision = Some(
            self.trial
                .as_ref()
                .map_or(self.revision, |trial| trial.backup.revision),
        );
    }

    /// Makes the game count as altered although no edit was made, for a game
    /// that exists only in memory (pasted text). Saving clears it.
    pub(crate) fn mark_altered(&mut self) {
        self.saved_revision = None;
    }

    /// The current revision of the game *as far as the "altered" flag is
    /// concerned*: it changes on every effective edit, undo and redo outside
    /// trial mode, and is restored when trial mode ends. Do not use it as a
    /// cache key; use [`GameSession::changes`].
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// A counter that grows each time the session starts a different game
    /// ([`GameSession::replace_game`]: new game, open, paste, next game). The
    /// front-end uses it to tell "another game" from "navigation" (no board
    /// animation between unrelated positions).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A monotonic counter that grows on every change of the game content,
    /// including edits inside trial mode, undo, redo, leaving trial mode and
    /// replacing the game. Observers (notation view, engine, caches) key on it.
    ///
    /// Any [`NodeId`] kept outside the session (comment editor, hover) must be
    /// dropped when this value changes through undo, redo or leaving trial
    /// mode (see the `NodeId` contract in `docs/arquitetura.md` 2.2.1).
    pub fn changes(&self) -> u64 {
        self.changes
    }

    /// Whether [`GameMessage::Undo`] would do something.
    pub fn can_undo(&self) -> bool {
        self.trial.is_none() && !self.undo.is_empty()
    }

    /// Whether [`GameMessage::Redo`] would do something.
    pub fn can_redo(&self) -> bool {
        self.trial.is_none() && !self.redo.is_empty()
    }

    /// Number of undo levels available, `0..=`[`UNDO_LIMIT`].
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Whether trial mode is on.
    pub fn in_trial(&self) -> bool {
        self.trial.is_some()
    }

    /// The open dialog, if any.
    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// The node an armed variation will hang from (see
    /// [`GameMessage::StartVariation`]).
    pub fn pending_variation(&self) -> Option<NodeId> {
        self.pending_variation
    }

    /// The open variation chooser, if any.
    pub fn chooser(&self) -> Option<&VariationChooser> {
        self.chooser.as_ref()
    }

    /// The keyboard entry state.
    pub fn move_entry(&self) -> &MoveEntry {
        &self.entry
    }

    /// The autoplay state.
    pub fn autoplay(&self) -> &Autoplay {
        &self.autoplay
    }

    /// The notice produced by the last message, if any.
    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// The edit preferences.
    pub fn settings(&self) -> &EditSettings {
        &self.settings
    }

    // ---- the funnel ----

    /// The single place where the [`Game`] is changed.
    ///
    /// Runs `op` on the game and the cursor. If `op` fails, game and cursor
    /// are restored and the matching [`Notice`] is set. If it succeeds but
    /// `Game::same_content` says nothing changed, nothing is recorded (no
    /// undo point, same revision). Otherwise (outside trial mode) the state
    /// from before goes to the undo stack, the redo stack is emptied and the
    /// revision changes. An effective edit also clears the keyboard entry,
    /// closes the chooser and disarms a pending variation.
    ///
    /// If the cursor node no longer exists afterwards, it moves to the last
    /// live node of its old path.
    pub(crate) fn edit<R>(
        &mut self,
        op: impl FnOnce(&mut Game, &mut NodeId) -> Result<R, GameError>,
    ) -> Result<R, GameError> {
        let before = self.snapshot();
        let old_path = self.game.path_to(self.cursor).unwrap_or_default();
        let mut cursor = self.cursor;
        let result = match op(&mut self.game, &mut cursor) {
            Ok(result) => result,
            Err(error) => {
                self.game = before.game;
                self.cursor = before.cursor;
                self.notice = Some(Notice::from_game_error(&error));
                return Err(error);
            }
        };
        if !self.game.contains(cursor) {
            cursor = last_live(&self.game, &old_path);
        }
        self.cursor = cursor;
        if self.game.same_content(&before.game) {
            return Ok(result);
        }
        self.changes = self.changes.saturating_add(1);
        if self.trial.is_none() {
            // Inside an edit group only the first edit leaves an undo point.
            let record = match &mut self.group {
                Some(pushed) => !std::mem::replace(pushed, true),
                None => true,
            };
            if record {
                self.push_undo(before);
            }
            self.redo.clear();
            self.revision = self.fresh_revision();
        }
        self.reset_interaction();
        Ok(result)
    }

    /// Opens an edit group: the edits made until [`GameSession::end_edit_group`]
    /// share one undo point, the one of the first of them (D469, the Scid's
    /// single `undoPoint` of an annotation run).
    pub(crate) fn begin_edit_group(&mut self) {
        self.group = Some(false);
    }

    /// Closes the edit group; later edits leave their own undo points.
    pub(crate) fn end_edit_group(&mut self) {
        self.group = None;
    }

    /// Marks the game as holding tactical exercises: the `T` flag waits for
    /// the game to be saved in a database (spec M4-6, D471).
    pub(crate) fn mark_tactics(&mut self) {
        self.tactics_pending = true;
    }

    /// Whether the game was marked as tactical exercises and the flag has
    /// not reached a database yet.
    pub fn tactics_pending(&self) -> bool {
        self.tactics_pending
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            game: self.game.clone(),
            cursor: self.cursor,
            revision: self.revision,
        }
    }

    fn push_undo(&mut self, snapshot: Snapshot) {
        self.undo.push_back(snapshot);
        while self.undo.len() > UNDO_LIMIT {
            self.undo.pop_front();
        }
    }

    fn fresh_revision(&mut self) -> Revision {
        self.revision_counter = self.revision_counter.saturating_add(1);
        Revision(self.revision_counter)
    }

    // ---- shared helpers ----

    /// Clears the keyboard entry, closes the chooser, disarms the variation.
    fn reset_interaction(&mut self) {
        self.entry.clear();
        self.chooser = None;
        self.pending_variation = None;
    }

    /// Moves the cursor (navigation): also resets the interaction state.
    fn move_to(&mut self, node: NodeId) {
        self.cursor = node;
        self.reset_interaction();
    }

    /// The children of `node`, or none for a stale id.
    fn children_of(&self, node: NodeId) -> &[NodeId] {
        self.game.children(node).unwrap_or(&[])
    }

    /// Opens a dialog; autoplay stops (spec rule 31).
    fn open_prompt(&mut self, prompt: Prompt) {
        self.prompt = Some(prompt);
        self.chooser = None;
        self.autoplay.running = false;
        self.autoplay.generation = self.autoplay.generation.wrapping_add(1);
    }
}

/// The deepest node of `path` (root-first) whose ancestors are all alive,
/// or the root.
fn last_live(game: &Game, path: &[NodeId]) -> NodeId {
    let mut last = game.root();
    for &id in path {
        if game.contains(id) {
            last = id;
        } else {
            break;
        }
    }
    last
}
