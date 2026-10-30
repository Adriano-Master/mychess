//! Typed status messages (the GUI translates them with Fluent).

use mychess_core::{GameError, MoveError};

/// Why the last message did nothing, or a remark about it.
///
/// Equivalent to the Scid's status bar and `tk_messageBox` texts, as data.
///
/// Not `#[non_exhaustive]`: the front-end matches it without a wildcard, so
/// the compiler points at every new variant that still needs a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The move is not legal in the position.
    IllegalMove,
    /// A null move was refused because the side to move is in check.
    NullMoveInCheck,
    /// A dialog is open and only its answer is accepted.
    PromptPending,
    /// A node id no longer names a node of the game.
    StaleNode,
    /// The operation needs a move, but the position is the game start.
    NoMoveHere,
    /// The node is not the head of a variation.
    NotAVariation,
    /// The cursor is not inside a variation.
    NotInVariation,
    /// Undo is disabled in trial mode.
    UndoDisabledInTrial,
    /// Redo is disabled in trial mode.
    RedoDisabledInTrial,
    /// The text could not be read as a move (clipped to 64 characters).
    UnreadableMove(String),
    /// No legal move starts with the typed text (clipped to 64 characters).
    MoveEntryNoMatch(String),
    /// The game has too many nodes.
    GameTooLarge,
    /// A removal found nothing to remove.
    NothingToStrip,
}

impl Notice {
    /// Maps an error of the core to its notice.
    pub(crate) fn from_game_error(error: &GameError) -> Notice {
        match error {
            GameError::IsRoot => Notice::NoMoveHere,
            GameError::NotVariation(_) => Notice::NotAVariation,
            GameError::IllegalMove(MoveError::NullMoveInCheck { .. }) => Notice::NullMoveInCheck,
            GameError::IllegalMove(_) => Notice::IllegalMove,
            GameError::TooManyNodes => Notice::GameTooLarge,
            // `InvalidNode` and any future variant: the id is unusable.
            _ => Notice::StaleNode,
        }
    }

    /// A no-match notice with the text clipped to 64 characters.
    pub(crate) fn unreadable_entry(text: &str) -> Notice {
        Notice::MoveEntryNoMatch(text.chars().take(64).collect())
    }

    /// An unreadable-move notice with the text clipped to 64 characters.
    pub(crate) fn unreadable(text: &str) -> Notice {
        Notice::UnreadableMove(text.chars().take(64).collect())
    }
}
