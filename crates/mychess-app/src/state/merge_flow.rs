//! The merge of a game into the current one (spec M7-6b, D3850-D3899):
//! opening the dialog, the answer of the reading, the preview, "OK".
//!
//! Everything here is bookkeeping and a copy of the game in memory, which
//! `update` may do. Reading the games is [`Effect::LoadGamesForMerge`].

use std::sync::Arc;

use mychess_core::{Game, MergeError, merge_comment_parts};

use super::AppState;
use crate::bases::SlotId;
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::AppNotice;
use crate::merge_game::{
    BATCH_MAX, BatchReport, MAX_END_PLY, MergeDialog, MergeLoadError, MergeMessage, MergeNotice,
    MergeNumbers, MergePrefs, MergeSource, MergeView, Outcome, PendingLoad, merge_all,
    preview_options,
};
use crate::notation::{NotationDoc, build_notation};
use crate::prefs::PrefChange;
use crate::prefs::PrefsMessage;
use crate::shared::SharedGame;

impl AppState {
    fn merge_notice(&mut self, notice: MergeNotice) {
        self.notice = Some(AppNotice::Merge(notice));
    }

    /// Handles a [`MergeMessage`].
    pub(crate) fn merge_message(&mut self, message: MergeMessage, effects: &mut Vec<Effect>) {
        match message {
            MergeMessage::Open { slot, number } => {
                self.merge_open(slot, vec![number], false, effects);
            }
            MergeMessage::OpenBatch { slot, numbers } => {
                if numbers.len() > BATCH_MAX {
                    self.merge_notice(MergeNotice::TooMany);
                    return;
                }
                self.merge_open(slot, numbers, true, effects);
            }
            MergeMessage::OpenFilter { slot, limit } => {
                let limit = usize::try_from(limit)
                    .unwrap_or(BATCH_MAX)
                    .clamp(1, BATCH_MAX);
                let Some(base) = self.bases.slot(slot) else {
                    self.merge_notice(MergeNotice::Closed);
                    return;
                };
                let numbers: Vec<u32> = base.filter().iter_games().take(limit).collect();
                self.merge_open(slot, numbers, true, effects);
            }
            MergeMessage::Loaded { request, games } => self.merge_loaded(request, games),
            MergeMessage::SetEndPly(end_ply) => self.merge_set_end_ply(end_ply, effects),
            MergeMessage::Apply { comment } => self.merge_apply(comment.as_deref(), effects),
            MergeMessage::Cancel => self.merge.cancel(),
        }
    }

    /// The action "merge the selected game of the list".
    pub(crate) fn merge_selected(&mut self, effects: &mut Vec<Effect>) {
        let slot = self.bases.current();
        let selected = self.bases.slot(slot).and_then(|s| s.list.selected);
        match selected {
            Some(number) => self.merge_open(slot, vec![number], false, effects),
            None => self.merge_notice(MergeNotice::NoGame),
        }
    }

    fn merge_open(
        &mut self,
        slot: SlotId,
        numbers: Vec<u32>,
        batch: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.modal_open() || self.refuse_edit() {
            return;
        }
        if self.session.in_trial() {
            self.merge_notice(MergeNotice::Trial);
            return;
        }
        if self.session.game().mainline().next().is_none() {
            self.merge_notice(MergeNotice::EmptyGame);
            return;
        }
        let Some(base) = self.bases.slot(slot) else {
            self.merge_notice(MergeNotice::Closed);
            return;
        };
        let total = base.total();
        // Only the current slot holds the game of the editor; the other slots
        // keep a stale number.
        let current = base.current_game.filter(|_| slot == self.bases.current());
        let source = base.source.clone();
        let mut skipped_same = 0;
        let mut skipped_invalid = 0;
        let mut wanted = Vec::with_capacity(numbers.len());
        for number in numbers {
            if number == 0 || number > total {
                if !batch {
                    self.merge_notice(MergeNotice::InvalidNumber);
                    return;
                }
                skipped_invalid += 1;
            } else if current == Some(number) {
                if !batch {
                    self.merge_notice(MergeNotice::SameGame);
                    return;
                }
                skipped_same += 1;
            } else if !wanted.contains(&number) {
                wanted.push(number);
            }
        }
        if wanted.is_empty() {
            if batch {
                let skipped = skipped_same + skipped_invalid;
                self.merge_notice(MergeNotice::BatchDone { merged: 0, skipped });
            } else {
                self.merge_notice(MergeNotice::InvalidNumber);
            }
            return;
        }
        // The game on the board becomes the user's again before it is
        // edited (a training session holds it aside).
        self.tactics_halt();
        let request = self.new_request();
        self.merge.dialog = None;
        self.merge.pending = Some(PendingLoad {
            request,
            slot,
            batch,
            skipped_same,
            skipped_invalid,
        });
        effects.push(Effect::LoadGamesForMerge {
            request,
            source,
            numbers: wanted,
        });
    }

    fn merge_loaded(
        &mut self,
        request: RequestId,
        games: Vec<(u32, Result<SharedGame, MergeLoadError>)>,
    ) {
        if self.merge.pending.as_ref().map(|p| p.request) != Some(request) {
            return;
        }
        let Some(pending) = self.merge.pending.take() else {
            return;
        };
        if self.bases.slot(pending.slot).is_none() {
            self.merge_notice(MergeNotice::Closed);
            return;
        }
        if self.modal_open() || self.refuse_edit() {
            return;
        }
        let mut sources = Vec::new();
        let mut unreadable = 0;
        let mut first_error = None;
        for (number, result) in games {
            match result {
                Ok(game) => sources.push(MergeSource { number, game }),
                Err(error) => {
                    unreadable += 1;
                    first_error.get_or_insert(error);
                }
            }
        }
        if !pending.batch
            && let Some(error) = first_error
        {
            self.merge_notice(error.into());
            return;
        }
        let skipped_unreadable = pending.skipped_invalid + unreadable;
        let end_ply = if pending.batch {
            None
        } else {
            self.prefs.merge.end_ply_option()
        };
        let parts = if pending.batch {
            None
        } else {
            sources.first().map(|s| merge_comment_parts(&s.game))
        };
        let mut dialog = MergeDialog {
            slot: pending.slot,
            batch: pending.batch,
            sources,
            end_ply,
            skipped_same: pending.skipped_same,
            skipped_unreadable,
            parts,
            outcome: Outcome {
                preview: Game::new(),
                doc: Arc::new(NotationDoc::default()),
                report: None,
                batch: BatchReport::default(),
            },
            changes: self.session.changes(),
        };
        if let Err(error) = self.merge_recompute(&mut dialog) {
            self.merge_notice(MergeNotice::from(&error));
            return;
        }
        if dialog.batch && dialog.outcome.batch.merged == 0 {
            let skipped = dialog.outcome.batch.skipped();
            self.merge_notice(MergeNotice::BatchDone { merged: 0, skipped });
            return;
        }
        if dialog.batch && !self.prefs.merge.confirm_batch {
            self.merge_commit(&dialog, None);
            return;
        }
        self.merge.dialog = Some(dialog);
    }

    /// The game of the editor with the merge done, on a copy.
    fn merge_compute(&self, dialog: &MergeDialog) -> Result<Outcome, MergeError> {
        let mut preview = self.session.game().clone();
        let (report, mut batch) = merge_all(
            &mut preview,
            &dialog.sources,
            dialog.batch,
            dialog.end_ply,
            None,
        )?;
        batch.skipped_same = dialog.skipped_same;
        batch.skipped_unreadable = batch
            .skipped_unreadable
            .saturating_add(dialog.skipped_unreadable);
        let doc = Arc::new(build_notation(&preview, &preview_options(), 0));
        Ok(Outcome {
            preview,
            doc,
            report,
            batch,
        })
    }

    /// Works the outcome of `dialog` out again against the game of the
    /// editor as it is now. On error the dialog is left as it was.
    fn merge_recompute(&mut self, dialog: &mut MergeDialog) -> Result<(), MergeError> {
        let outcome = self.merge_compute(dialog)?;
        self.merge.doc_builds = self.merge.doc_builds.saturating_add(1);
        dialog.outcome = outcome;
        dialog.changes = self.session.changes();
        Ok(())
    }

    /// How many notation documents of the preview were built since the
    /// program started. The view reads the cached one, so this only grows
    /// when the preview changes.
    pub fn merge_doc_builds(&self) -> u32 {
        self.merge.doc_builds
    }

    /// Keeps the dialog honest after every message: when the game of the
    /// editor changed under it (a remote move, an observed game) the preview
    /// is worked out again, and the dialog closes with a notice if the merge
    /// no longer works (spec D3884).
    pub(crate) fn merge_guard(&mut self) {
        let stale = self
            .merge
            .dialog
            .as_ref()
            .is_some_and(|d| d.changes != self.session.changes());
        if !stale {
            return;
        }
        let Some(mut dialog) = self.merge.dialog.take() else {
            return;
        };
        match self.merge_recompute(&mut dialog) {
            Ok(()) if !dialog.batch || dialog.outcome.batch.merged > 0 => {
                self.merge.dialog = Some(dialog);
            }
            Ok(()) => self.merge_notice(MergeNotice::NoMatch),
            Err(error) => self.merge_notice(MergeNotice::from(&error)),
        }
    }

    fn merge_set_end_ply(&mut self, end_ply: Option<u32>, effects: &mut Vec<Effect>) {
        let Some(mut dialog) = self.merge.dialog.take() else {
            return;
        };
        if dialog.batch {
            self.merge.dialog = Some(dialog);
            return;
        }
        let previous = dialog.end_ply;
        dialog.end_ply = end_ply.map(|n| n.clamp(1, MAX_END_PLY));
        if let Err(error) = self.merge_recompute(&mut dialog) {
            // Button, preview and "OK" must tell the same story: the old
            // outcome stays with the old ply.
            dialog.end_ply = previous;
            self.merge.dialog = Some(dialog);
            self.merge_notice(MergeNotice::from(&error));
            return;
        }
        let prefs = MergePrefs {
            end_ply: dialog.end_ply.unwrap_or(0),
            ..*self.prefs.merge
        };
        self.merge.dialog = Some(dialog);
        if prefs != *self.prefs.merge {
            self.prefs_message(PrefsMessage::Set(PrefChange::Merge(prefs)), effects);
        }
    }

    fn merge_apply(&mut self, comment: Option<&str>, _effects: &mut Vec<Effect>) {
        // The dialog stays until the merge is sure to happen: a refusal (an
        // annotation, a game against an engine) lets the user try again or
        // cancel.
        if self.merge.dialog.is_none() || self.refuse_edit() {
            return;
        }
        if self.session.in_trial() {
            self.merge_notice(MergeNotice::Trial);
            return;
        }
        let Some(dialog) = self.merge.dialog.take() else {
            return;
        };
        self.merge_commit(&dialog, comment);
    }

    /// Merges into the game of the editor as one step of the undo.
    fn merge_commit(&mut self, dialog: &MergeDialog, comment: Option<&str>) {
        if self.session.in_trial() {
            self.merge_notice(MergeNotice::Trial);
            return;
        }
        let mut failure = None;
        let mut counts = None;
        let result = self.session.edit(|game, _cursor| {
            match merge_all(game, &dialog.sources, dialog.batch, dialog.end_ply, comment) {
                Ok((_, batch)) => counts = Some(batch),
                Err(MergeError::Game(error)) => return Err(error),
                Err(error) => failure = Some(error),
            }
            Ok(())
        });
        if result.is_err() {
            self.merge_notice(MergeNotice::Failed);
        } else if let Some(error) = failure {
            self.merge_notice(MergeNotice::from(&error));
        } else if dialog.batch
            && let Some(mut batch) = counts
        {
            batch.skipped_same = dialog.skipped_same;
            batch.skipped_unreadable = batch
                .skipped_unreadable
                .saturating_add(dialog.skipped_unreadable);
            self.merge_notice(MergeNotice::BatchDone {
                merged: batch.merged,
                skipped: batch.skipped(),
            });
        }
    }

    /// What the merge dialog draws, if it is open.
    pub fn merge_view(&self) -> Option<MergeView<'_>> {
        let dialog = self.merge.dialog.as_ref()?;
        Some(MergeView {
            batch: dialog.batch,
            slot: dialog.slot,
            numbers: MergeNumbers(&dialog.sources),
            end_ply: dialog.end_ply,
            original: self.session.game(),
            preview: &dialog.outcome.preview,
            preview_doc: &dialog.outcome.doc,
            report: dialog.outcome.report,
            batch_report: dialog.outcome.batch,
            comment_parts: dialog.parts.as_ref(),
        })
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::SanMode;

    use super::*;

    fn game(sans: &[&str]) -> Game {
        let mut game = Game::new();
        let mut at = game.root();
        for san in sans {
            let mv = game
                .position_at(at)
                .expect("position")
                .parse_san(san, SanMode::Strict)
                .expect("legal move")
                .mv;
            at = game.add_move(at, mv).expect("added");
        }
        game
    }

    #[test]
    fn a_failed_recompute_of_the_end_ply_keeps_the_old_ply_and_outcome() {
        let mut state = AppState::default();
        state.session.replace_game(game(&["e4", "e5"]));
        let request = state.new_request();
        state.merge.pending = Some(PendingLoad {
            request,
            slot: SlotId::CLIPBASE,
            batch: false,
            skipped_same: 0,
            skipped_invalid: 0,
        });
        let other = game(&["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        state.merge_loaded(request, vec![(2, Ok(SharedGame::new(other)))]);
        let (end_ply, nodes) = {
            let dialog = state.merge.dialog.as_ref().expect("dialog");
            (dialog.end_ply, dialog.outcome.preview.node_count())
        };
        // Nothing left to merge: the recompute refuses.
        state.merge.dialog.as_mut().expect("dialog").sources.clear();
        let mut effects = Vec::new();
        state.merge_set_end_ply(Some(6), &mut effects);
        let dialog = state.merge.dialog.as_ref().expect("dialog stays");
        assert_eq!(dialog.end_ply, end_ply, "the button keeps the old ply");
        assert_eq!(dialog.outcome.preview.node_count(), nodes);
        assert_eq!(
            state.notice,
            Some(AppNotice::Merge(MergeNotice::NothingToAdd))
        );
        assert!(effects.is_empty(), "the refused ply is not remembered");
    }
}
