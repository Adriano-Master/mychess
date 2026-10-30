//! Playing a move at the cursor (spec rules 32-43).

use mychess_core::{Move, NodeId, Role, Square};

use super::message::{PlayOptions, ReplaceChoice};
use super::{GameSession, Notice};

/// A new move waiting for the "replace move" answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingMove {
    /// The node the move would be played from (the cursor).
    pub parent: NodeId,
    /// The move.
    pub mv: Move,
}

/// A modal question to the user. While one is open, only its answer (or
/// dismissal) is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// The position already has continuations: replace, add, or cancel.
    ReplaceMove(PendingMove),
    /// A pawn move that promotes, without the piece.
    ChoosePromotion {
        /// Origin square.
        from: Square,
        /// Destination square.
        to: Square,
        /// The move was asked as a forced variation (middle click).
        force_variation: bool,
    },
    /// Removing comments, variations or moves: how many items go, and
    /// whether to go on (answer with
    /// [`GameMessage::AnswerStrip`](super::GameMessage::AnswerStrip)).
    ConfirmStrip {
        /// What is removed.
        kind: super::StripKind,
        /// How many comments, variations or moves.
        count: usize,
    },
}

/// What to do with a new move at a position that has continuations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddAction {
    /// Append as a variation (or as the continuation, at a leaf).
    AddVariation,
    /// Drop the following moves and variations, then append.
    Replace,
    /// Append and make it the main line.
    NewMainLine,
}

impl GameSession {
    pub(super) fn play_null_move(&mut self) {
        self.play_move(Move::Null, PlayOptions::default());
    }

    /// The entry point of every move: legality, existing move, then the
    /// decision table of the spec (rule 38).
    pub(super) fn play_move(&mut self, mv: Move, options: PlayOptions) {
        let cursor = self.cursor;
        let Ok(position) = self.game.position_at(cursor) else {
            self.notice = Some(Notice::StaleNode);
            return;
        };
        let legal = position.is_legal(mv);
        if !legal {
            self.notice = Some(if mv.is_null() {
                Notice::NullMoveInCheck
            } else {
                Notice::IllegalMove
            });
            return;
        }
        if self.capture_moves {
            self.captured = Some(mv);
            return;
        }
        match self.game.find_child(cursor, mv) {
            Ok(Some(child)) => {
                self.move_to(child);
                return;
            }
            Ok(None) => {}
            Err(error) => {
                self.notice = Some(Notice::from_game_error(&error));
                return;
            }
        }
        if self.pending_variation == Some(cursor) || self.children_of(cursor).is_empty() {
            self.apply_add(cursor, mv, AddAction::AddVariation);
            return;
        }
        let action = if options.force_variation {
            AddAction::AddVariation
        } else if !self.settings.ask_to_replace_moves || self.trial.is_some() {
            AddAction::Replace
        } else {
            self.open_prompt(Prompt::ReplaceMove(PendingMove { parent: cursor, mv }));
            return;
        };
        self.apply_add(cursor, mv, action);
    }

    fn apply_add(&mut self, parent: NodeId, mv: Move, action: AddAction) {
        // The error (if any) already became a notice inside the funnel.
        drop(self.edit(|game, cursor| {
            if action == AddAction::Replace {
                game.truncate_after(parent)?;
            }
            let node = game.add_move(parent, mv)?;
            if action == AddAction::NewMainLine && game.is_variation(node)? {
                game.promote_variation(node)?;
            }
            *cursor = node;
            Ok(())
        }));
    }

    pub(super) fn answer_replace(&mut self, choice: ReplaceChoice) {
        let pending = match &self.prompt {
            Some(Prompt::ReplaceMove(pending)) => *pending,
            _ => return,
        };
        self.prompt = None;
        match choice {
            ReplaceChoice::Cancel => self.entry.clear(),
            ReplaceChoice::Replace => {
                self.apply_add(pending.parent, pending.mv, AddAction::Replace);
            }
            ReplaceChoice::NewMainLine => {
                self.apply_add(pending.parent, pending.mv, AddAction::NewMainLine);
            }
            ReplaceChoice::AddVariation => {
                self.apply_add(pending.parent, pending.mv, AddAction::AddVariation);
            }
            ReplaceChoice::Trial => {
                self.begin_trial();
                self.apply_add(pending.parent, pending.mv, AddAction::Replace);
            }
        }
    }

    pub(super) fn answer_promotion(&mut self, role: Option<Role>) {
        let (from, to, force_variation) = match &self.prompt {
            Some(Prompt::ChoosePromotion {
                from,
                to,
                force_variation,
            }) => (*from, *to, *force_variation),
            _ => return,
        };
        self.prompt = None;
        match role {
            None => {}
            Some(role @ (Role::Queen | Role::Rook | Role::Bishop | Role::Knight)) => {
                self.play_squares(from, to, Some(role), PlayOptions { force_variation });
            }
            Some(_) => self.notice = Some(Notice::IllegalMove),
        }
    }

    pub(super) fn dismiss_prompt(&mut self) {
        if self.prompt.take().is_some() {
            self.entry.clear();
        }
    }

    pub(super) fn play_squares(
        &mut self,
        from: Square,
        to: Square,
        promotion: Option<Role>,
        options: PlayOptions,
    ) {
        let Ok(position) = self.game.position_at(self.cursor) else {
            self.notice = Some(Notice::StaleNode);
            return;
        };
        let is_king = |square: Square| {
            position
                .piece_at(square)
                .is_some_and(|piece| piece.role == Role::King)
        };
        // A click or drop on the same square is not a move.
        if from == to {
            return;
        }
        // Scid: king takes king is the way to enter a null move.
        if is_king(from) && is_king(to) {
            self.play_move(Move::Null, options);
            return;
        }
        if promotion.is_none() && position.find_move(from, to, Some(Role::Queen)).is_some() {
            self.open_prompt(Prompt::ChoosePromotion {
                from,
                to,
                force_variation: options.force_variation,
            });
            return;
        }
        match position.find_move(from, to, promotion) {
            Some(mv) => self.play_move(mv, options),
            None => self.notice = Some(Notice::IllegalMove),
        }
    }

    pub(super) fn start_variation(&mut self) {
        let cursor = self.cursor;
        if !self.children_of(cursor).is_empty() {
            self.pending_variation = Some(cursor);
            return;
        }
        // A leaf: the variation is an alternative to the last move, so it
        // hangs from the parent (Scid: `sc_var create` backs up one move).
        match self.game.parent(cursor) {
            Ok(Some(parent)) => {
                self.cursor = parent;
                self.reset_interaction();
                self.pending_variation = Some(parent);
            }
            _ => self.notice = Some(Notice::NoMoveHere),
        }
    }
}
