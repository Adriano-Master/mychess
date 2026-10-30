//! Messages understood by [`GameSession`](super::GameSession).

use std::time::Duration;

use mychess_core::{Move, Nag, NodeId, Role, Square};

use super::analysis::EngineLineToAdd;
use super::settings::EditSettings;
use super::strip::StripKind;

/// An event for the game editing session.
///
/// Spec: `docs/specs/m2-1-edicao-de-partida.md`. Messages that carry a
/// [`NodeId`] never panic on a stale id: they produce
/// [`Notice::StaleNode`](super::Notice::StaleNode).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GameMessage {
    // ---- 1. Navigation ----
    /// Moves the cursor.
    Navigate(Nav),
    /// Opens the variation chooser (needs at least two continuations).
    OpenVariationChooser,
    /// Moves the chooser highlight by `delta` rows, saturating at the ends.
    ChooserMove(isize),
    /// Enters a continuation: `None` is the highlighted row, `Some(i)` a
    /// digit key (`v` is `Some(1)`).
    ChooserConfirm(Option<usize>),
    /// Closes the chooser without moving.
    CloseVariationChooser,
    /// Starts autoplay if stopped, stops it if running.
    ToggleAutoplay,
    /// Starts autoplay (one immediate step, then one tick per step).
    StartAutoplay,
    /// Stops autoplay.
    StopAutoplay,
    /// Sets the autoplay delay, clamped to
    /// [`MIN_AUTOPLAY_DELAY`](super::MIN_AUTOPLAY_DELAY)`..=`
    /// [`MAX_AUTOPLAY_DELAY`](super::MAX_AUTOPLAY_DELAY).
    SetAutoplayDelay(Duration),
    /// The timer of [`Effect::ScheduleAutoplayTick`](crate::Effect) fired.
    AutoplayTick {
        /// Generation the tick was scheduled for; stale ones are ignored.
        generation: u64,
    },

    // ---- 2. Playing a move ----
    /// Plays a move at the cursor.
    PlayMove {
        /// The move (checked for legality).
        mv: Move,
        /// How to treat a move that already has continuations.
        options: PlayOptions,
    },
    /// Plays the move between two squares (drag and drop or clicks).
    PlaySquares {
        /// Origin square.
        from: Square,
        /// Destination square.
        to: Square,
        /// Promotion piece, when the GUI already knows it.
        promotion: Option<Role>,
        /// How to treat a move that already has continuations.
        options: PlayOptions,
    },
    /// Plays the null move (`--`).
    PlayNullMove,
    /// Arms a variation: the next new move becomes a variation of the cursor
    /// position (Scid: Ctrl+A).
    StartVariation,
    /// Answer to the "replace move" prompt.
    AnswerReplace(ReplaceChoice),
    /// Answer to the promotion prompt; `None` cancels.
    AnswerPromotion(Option<Role>),
    /// Closes the open prompt as if cancelled.
    DismissPrompt,

    // ---- 3. Keyboard move entry ----
    /// A typed character.
    MoveEntryChar(char),
    /// A character typed at the board. Like [`GameMessage::MoveEntryChar`],
    /// but a printable key that no legal move continues is refused with
    /// [`Notice::MoveEntryNoMatch`](super::Notice::MoveEntryNoMatch) (spec
    /// M2-4, rule 63).
    MoveEntryTyped(char),
    /// Backspace.
    MoveEntryBackspace,
    /// Enter.
    MoveEntryCommit,
    /// Escape: empties the entry.
    MoveEntryClear,
    /// Free move text (SAN or UCI), for pasting, engines and tests.
    EnterMoveText(String),

    // ---- 4. Structural editing and annotation ----
    /// Makes a variation the main continuation.
    PromoteVariation(VariationTarget),
    /// Makes a variation the first one (`children[1]`).
    MakeFirstVariation(VariationTarget),
    /// Deletes a variation with its subtree.
    DeleteVariation(VariationTarget),
    /// Deletes everything after the cursor (the cursor move stays).
    TruncateEnd,
    /// Deletes the cursor move and everything after it, keeping siblings
    /// (no equivalent in Scid).
    TruncateFrom,
    /// Makes the cursor position the start of a new game.
    TruncateStart,
    /// Removes every comment (NAGs stay).
    StripComments,
    /// Removes every variation.
    StripVariations,
    /// Removes every NAG (comments stay).
    StripNags,
    /// Asks to remove comments, variations or moves: counts what would go
    /// and, with `confirm`, opens `Prompt::ConfirmStrip`; without it (the
    /// user turned the confirmation off) removes at once. Nothing to remove
    /// gives `Notice::NothingToStrip`.
    RequestStrip {
        /// What to remove.
        kind: StripKind,
        /// Ask first (the `confirm_destructive` preference, on by default).
        confirm: bool,
    },
    /// Answer to the confirmation of [`GameMessage::RequestStrip`].
    AnswerStrip(bool),
    /// Sets the comment after the move of `node` (initial comment for the
    /// root). Blank text removes it.
    SetComment {
        /// The node.
        node: NodeId,
        /// The text, stored literally.
        text: String,
    },
    /// Sets the comment before the move of `node`.
    SetCommentBefore {
        /// The node.
        node: NodeId,
        /// The text, stored literally.
        text: String,
    },
    /// Adds a NAG to the cursor move.
    AddNag(Nag),
    /// Removes the NAG if present, adds it otherwise.
    ToggleNag(Nag),
    /// Like [`GameMessage::ToggleNag`] on the move of `node` (the context
    /// menu of the move list); the cursor does not move.
    ToggleNagAt {
        /// The move to annotate.
        node: NodeId,
        /// The NAG.
        nag: Nag,
    },
    /// Removes a NAG from the cursor move.
    RemoveNag(Nag),
    /// Removes every NAG from the cursor move.
    ClearNags,
    /// Replaces the NAGs of the cursor move.
    SetNags(Vec<Nag>),

    // ---- 5. History ----
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Enters trial mode (edits are reverted on exit).
    EnterTrial {
        /// Also add a null move.
        null_move: bool,
    },
    /// Leaves trial mode, restoring the game.
    ExitTrial,
    /// Enters or leaves trial mode.
    ToggleTrial {
        /// Also add a null move when entering.
        null_move: bool,
    },

    /// New edit preferences.
    ApplySettings(EditSettings),

    // ---- 6. Engine lines (spec M4-4, D440) ----
    /// Adds the line of an engine as a variation of the position of `node`
    /// (the cursor when the button was pressed). One undo point; the cursor
    /// does not move. The comment goes before the first new move. See
    /// [`GameSession`](super::GameSession) for the end-of-line rules.
    AddEngineLine {
        /// The position the line starts from.
        node: NodeId,
        /// The moves.
        pv: Vec<Move>,
        /// The comment (`depth:score`).
        comment: Option<String>,
    },
    /// Adds several lines in one undo point ("add all variations").
    AddEngineLines {
        /// The position the lines start from.
        node: NodeId,
        /// The lines, in order.
        lines: Vec<EngineLineToAdd>,
    },
    /// Puts `text` in front of the comment after the move of `node`
    /// ("add score").
    PrependComment {
        /// The node (the root for the initial comment).
        node: NodeId,
        /// The text.
        text: String,
    },
}

impl GameMessage {
    /// Whether the message is accepted while a prompt is open.
    pub(crate) fn allowed_with_prompt(&self) -> bool {
        matches!(
            self,
            GameMessage::AnswerReplace(_)
                | GameMessage::AnswerPromotion(_)
                | GameMessage::AnswerStrip(_)
                | GameMessage::DismissPrompt
                | GameMessage::ApplySettings(_)
        )
    }
}

/// A cursor movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Nav {
    /// To the root.
    Start,
    /// To the leaf of the main line.
    GameEnd,
    /// Start of the current line; a second press climbs one more level.
    Home,
    /// End of the current line, then of the main line.
    End,
    /// Back `n` moves; stops at the position a variation starts from.
    Back(u32),
    /// Forward `n` moves along the first continuation.
    Forward(u32),
    /// Leaves the current variation (to the position it starts from).
    ExitVariation,
    /// Enters continuation `i` of the cursor (0 is the main one).
    EnterVariation(usize),
    /// To a node.
    GoToNode(NodeId),
    /// To the main-line node at this ply (past the end: the leaf).
    GoToMainlinePly(u32),
    /// To the position after White's move `n` in the main line.
    GoToMoveNumber(u32),
    /// To the position after Black's move `n` in the main line (`12...`).
    GoToBlackMoveNumber(u32),
}

/// How a move that already has continuations is handled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayOptions {
    /// Always add a variation without asking (middle click).
    pub force_variation: bool,
}

/// Choice in the "move already exists" dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceChoice {
    /// Discard the following moves and variations.
    Replace,
    /// Play the move as the main line; the old one becomes a variation.
    NewMainLine,
    /// Add a variation.
    AddVariation,
    /// Enter trial mode and replace there.
    Trial,
    /// Do nothing.
    Cancel,
}

/// Which variation an operation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariationTarget {
    /// The head of this variation.
    Node(NodeId),
    /// The variation the cursor is in.
    Current,
}
