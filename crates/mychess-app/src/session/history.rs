//! Undo, redo and trial mode (spec rules 69-76).

use mychess_core::Move;

use super::message::PlayOptions;
use super::settings::EditSettings;
use super::{GameSession, MAX_AUTOPLAY_DELAY, MIN_AUTOPLAY_DELAY, Notice, Snapshot};

/// Trial mode: edits happen in place and are reverted by restoring `backup`.
#[derive(Debug)]
pub(super) struct Trial {
    pub(super) backup: Snapshot,
}

impl GameSession {
    /// Brings back a stored state and closes every pending interaction.
    fn restore(&mut self, snapshot: Snapshot) {
        self.cursor = if snapshot.game.contains(snapshot.cursor) {
            snapshot.cursor
        } else {
            snapshot.game.root()
        };
        self.game = snapshot.game;
        self.revision = snapshot.revision;
        self.changes = self.changes.saturating_add(1);
        self.prompt = None;
        self.reset_interaction();
    }

    pub(super) fn undo(&mut self) {
        if self.trial.is_some() {
            self.notice = Some(Notice::UndoDisabledInTrial);
            return;
        }
        let Some(previous) = self.undo.pop_back() else {
            return;
        };
        let current = self.snapshot();
        self.redo.push(current);
        self.restore(previous);
    }

    pub(super) fn redo(&mut self) {
        if self.trial.is_some() {
            self.notice = Some(Notice::RedoDisabledInTrial);
            return;
        }
        let Some(next) = self.redo.pop() else {
            return;
        };
        let current = self.snapshot();
        self.push_undo(current);
        self.restore(next);
    }

    /// Keeps the current state as the backup and turns trial mode on.
    pub(super) fn begin_trial(&mut self) {
        if self.trial.is_some() {
            return;
        }
        self.trial = Some(Trial {
            backup: self.snapshot(),
        });
        self.prompt = None;
        self.reset_interaction();
    }

    pub(super) fn enter_trial(&mut self, null_move: bool) {
        if self.trial.is_some() {
            return;
        }
        self.begin_trial();
        if null_move {
            // A continuation at a leaf, a variation otherwise; a refused
            // null move (check) only leaves its notice.
            self.play_move(
                Move::Null,
                PlayOptions {
                    force_variation: true,
                },
            );
        }
    }

    pub(super) fn exit_trial(&mut self) {
        if let Some(trial) = self.trial.take() {
            self.restore(trial.backup);
        }
    }

    pub(super) fn apply_settings(&mut self, settings: EditSettings) {
        let delay = settings
            .autoplay_delay
            .clamp(MIN_AUTOPLAY_DELAY, MAX_AUTOPLAY_DELAY);
        self.autoplay.delay = delay;
        if !settings.move_entry.enabled {
            self.entry.clear();
        }
        self.settings = settings;
    }
}
