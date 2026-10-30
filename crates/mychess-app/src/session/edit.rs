//! Structural editing: variations, truncating, stripping (spec rules 55-61).

use mychess_core::NodeId;

use super::message::VariationTarget;
use super::{GameSession, Notice};

impl GameSession {
    /// Resolves a target to the head of a variation, or sets the notice.
    fn resolve_variation(&mut self, target: VariationTarget) -> Option<NodeId> {
        match target {
            VariationTarget::Node(id) => match self.game.is_variation(id) {
                Ok(true) => Some(id),
                Ok(false) => {
                    self.notice = Some(Notice::NotAVariation);
                    None
                }
                Err(_) => {
                    self.notice = Some(Notice::StaleNode);
                    None
                }
            },
            VariationTarget::Current => {
                let head = self.line_head(self.cursor);
                if head.is_none() {
                    self.notice = Some(Notice::NotInVariation);
                }
                head
            }
        }
    }

    pub(super) fn promote_variation(&mut self, target: VariationTarget) {
        if let Some(head) = self.resolve_variation(target) {
            drop(self.edit(|game, _| game.promote_variation(head)));
        }
    }

    pub(super) fn make_first_variation(&mut self, target: VariationTarget) {
        if let Some(head) = self.resolve_variation(target) {
            drop(self.edit(|game, _| game.make_first(head)));
        }
    }

    pub(super) fn delete_variation(&mut self, target: VariationTarget) {
        // The funnel moves a cursor inside the deleted subtree to the
        // position the variation started from.
        if let Some(head) = self.resolve_variation(target) {
            drop(self.edit(|game, _| game.delete_variation(head)));
        }
    }

    /// Deletes everything after the cursor; the cursor move stays (Scid:
    /// `Game::Truncate`).
    pub(super) fn truncate_end(&mut self) {
        drop(self.edit(|game, cursor| game.truncate_after(*cursor)));
    }

    /// Deletes the cursor move and what follows, keeping its siblings.
    pub(super) fn truncate_from(&mut self) {
        drop(self.edit(|game, cursor| game.truncate_from(*cursor)));
    }

    /// Makes the cursor position the start of a new game (new identity).
    pub(super) fn truncate_start(&mut self) {
        if self.cursor == self.game.root() {
            return;
        }
        drop(self.edit(|game, cursor| {
            let truncated = game.truncate_start(*cursor)?;
            *game = truncated;
            *cursor = game.root();
            Ok(())
        }));
    }

    pub(super) fn strip_comments(&mut self) {
        drop(self.edit(|game, _| {
            game.strip_comments();
            Ok(())
        }));
    }

    pub(super) fn strip_variations(&mut self) {
        drop(self.edit(|game, _| {
            game.strip_variations();
            Ok(())
        }));
    }

    pub(super) fn strip_nags(&mut self) {
        drop(self.edit(|game, _| {
            game.strip_nags();
            Ok(())
        }));
    }
}
