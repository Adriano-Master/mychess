//! The notation panel's view-model: the game as blocks of inline items.
//!
//! [`build_notation`] turns a [`Game`] and the [`NotationOptions`] into a
//! [`NotationDoc`]. The front-end only paints it and maps clicks back to
//! [`NodeId`]s and [`CommentTarget`]s, so the numbering, NAG spacing and
//! indentation rules (the Scid's `Game::WriteMoveList`, `src/game.cpp:3088`)
//! are plain functions that can be tested without a window.
//!
//! The document is split in *blocks* (a header-less paragraph each) so the
//! front-end can redo the text shaping only for a block whose
//! [`Block::rev`] changed; see [`NotationDoc::keep_revisions`].
//!
//! Spec: `docs/specs/m2-4-notacao-e-dialogos.md` (D160-D170, section 3.1).

mod build;
mod header;
mod opening;
mod table;
#[cfg(test)]
mod table_tests;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::fmt::Write as _;

use mychess_core::{Game, GameResult, Nag, NodeId};
use serde::{Serialize, Serializer};

pub use header::HeaderLine;
pub use opening::opening_along_path;
pub use table::{
    Cell, MAX_CARD_INDENT, MovePlace, MoveTimes, NotationTable, Row, RowContent, RowKey, RowKind,
};

/// Deepest indentation level drawn (the Scid has no limit; D163).
pub const MAX_INDENT: u8 = 6;

/// How variations and comments are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotationLayout {
    /// One row per full move (`number | white | black`), comments under the
    /// row and variations as cards (M2-5 D241). The document has the same
    /// blocks as [`NotationLayout::Indented`] with the comments indented and
    /// no space after move numbers; [`NotationTable`] is a projection of it.
    #[default]
    Table,
    /// Each variation is a block of its own, indented (the Scid's default).
    Indented,
    /// Everything flows in one paragraph.
    Inline,
}

impl NotationLayout {
    /// The spelling used in the preferences file.
    pub fn as_str(self) -> &'static str {
        match self {
            NotationLayout::Table => "table",
            NotationLayout::Indented => "indented",
            NotationLayout::Inline => "inline",
        }
    }

    /// Reads [`NotationLayout::as_str`].
    pub fn from_name(text: &str) -> Option<NotationLayout> {
        match text {
            "table" => Some(NotationLayout::Table),
            "indented" => Some(NotationLayout::Indented),
            "inline" => Some(NotationLayout::Inline),
            _ => None,
        }
    }
}

/// How much of the game header is shown above the moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeaderMode {
    /// No header (the default since M2-5: the tags are in the information
    /// tab).
    #[default]
    Hidden,
    /// Three lines: players, event, start position (`WritePGN`'s short form).
    Short,
    /// One line per tag.
    Full,
}

impl HeaderMode {
    /// The spelling used in the preferences file.
    pub fn as_str(self) -> &'static str {
        match self {
            HeaderMode::Hidden => "hidden",
            HeaderMode::Short => "short",
            HeaderMode::Full => "full",
        }
    }

    /// Reads [`HeaderMode::as_str`].
    pub fn from_name(text: &str) -> Option<HeaderMode> {
        match text {
            "hidden" => Some(HeaderMode::Hidden),
            "short" => Some(HeaderMode::Short),
            "full" => Some(HeaderMode::Full),
            _ => None,
        }
    }
}

/// Whether hovering a move shows the position in a small board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoverPreview {
    /// Never.
    Off,
    /// After the pointer rested this many milliseconds on a move.
    Delay(u32),
}

impl HoverPreview {
    /// Longest delay kept, in milliseconds.
    pub const MAX_DELAY_MS: u32 = 5000;

    /// The delay in milliseconds, 0 for [`HoverPreview::Off`] (the form
    /// stored in the preferences file).
    pub fn as_millis(self) -> u32 {
        match self {
            HoverPreview::Off => 0,
            HoverPreview::Delay(ms) => ms,
        }
    }

    /// Reads [`HoverPreview::as_millis`]; 0 is off and large values are
    /// limited to [`HoverPreview::MAX_DELAY_MS`].
    pub fn from_millis(ms: u32) -> HoverPreview {
        if ms == 0 {
            HoverPreview::Off
        } else {
            HoverPreview::Delay(ms.min(HoverPreview::MAX_DELAY_MS))
        }
    }
}

impl Serialize for HoverPreview {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.as_millis())
    }
}

/// What the notation panel shows (D162). Defaults are the Scid's
/// (`tcl/start.tcl:481-490`) plus the options that program lacks; since M2-5
/// the layout is [`NotationLayout::Table`] and the header is hidden (D241,
/// D249).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NotationOptions {
    /// Variations on blocks of their own, or everything in one paragraph.
    pub layout: NotationLayout,
    /// Put the comments of the main line on blocks of their own (only with
    /// [`NotationLayout::Indented`]).
    pub indent_comments: bool,
    /// NAGs as symbols (`!`, `+-`) instead of `$n`.
    pub symbolic_nags: bool,
    /// Write `12. e4` instead of `12.e4`.
    pub move_number_space: bool,
    /// How much of the header to show.
    pub header: HeaderMode,
    /// Hide `[%...]` commands in comments.
    pub strip_marks: bool,
    /// Draw the main line in bold (a paint hint; it does not change the
    /// document).
    pub bold_main_line: bool,
    /// Show comments **and NAGs** (the Scid's `-comments 0` hides both).
    pub show_comments: bool,
    /// Show variations.
    pub show_variations: bool,
    /// Show the result at the end.
    pub show_result: bool,
    /// Board preview when hovering a move.
    #[serde(rename = "hover_preview_ms")]
    pub hover_preview: HoverPreview,
    /// Draw a piece symbol before the SAN (`♞f3`); a paint hint, the document
    /// keeps the English SAN (M2-5 D243).
    pub figurines: bool,
    /// Show the time spent per move beside the table when the game has
    /// times (M2-5 D250); a paint hint.
    pub show_time_column: bool,
}

impl Default for NotationOptions {
    fn default() -> NotationOptions {
        NotationOptions {
            layout: NotationLayout::Table,
            indent_comments: true,
            symbolic_nags: true,
            move_number_space: false,
            header: HeaderMode::Hidden,
            strip_marks: false,
            bold_main_line: false,
            show_comments: true,
            show_variations: true,
            show_result: true,
            hover_preview: HoverPreview::Delay(600),
            figurines: true,
            show_time_column: true,
        }
    }
}

/// The comment a click edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentTarget {
    /// The comment after the move of this node (the initial comment for the
    /// root).
    After(NodeId),
    /// The comment before the move of this node, written right after the
    /// `(` that opens its variation.
    Before(NodeId),
    /// The comment of the game, before move 1.
    Initial,
}

/// What a block holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockKind {
    /// A stretch of the main line.
    Main,
    /// A variation, or the continuation of one after a sub-variation.
    Variation,
    /// A comment on a block of its own.
    Comment,
}

/// A stable identity for a block: the kind and the node of its first move
/// (or the node a comment block belongs to). Blocks of two documents with
/// the same key are the same paragraph, edited or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockKey {
    /// The kind of block.
    pub kind: BlockKind,
    /// The first move of the block, `None` for the placeholder of a pending
    /// variation.
    pub node: Option<NodeId>,
}

/// One item of a block. Items are laid out one after another with no
/// implicit space: every blank is an explicit [`Inline::Space`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Inline {
    /// A move number: `12.`, `12...` or, with the space option and Black to
    /// move, `12. ...`. Not clickable.
    Number(Box<str>),
    /// A move in SAN.
    Move {
        /// The node the move leads to.
        node: NodeId,
        /// The move in SAN (`--` for the null move).
        san: Box<str>,
        /// How many variations deep the move is (0 on the main line),
        /// saturating at 255.
        depth: u8,
        /// Whether the move is inside a variation.
        in_variation: bool,
    },
    /// A NAG of the move of `node`; its text is [`nag_text`].
    Nag {
        /// The annotated node.
        node: NodeId,
        /// The NAG.
        nag: Nag,
    },
    /// A comment, after the options (marks removed, ends trimmed).
    Comment {
        /// What a click edits.
        target: CommentTarget,
        /// The text.
        text: Box<str>,
    },
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// A blank.
    Space,
    /// The transient `( ... )` of a variation that was started
    /// (`Ctrl+A`) but has no move yet; `after` is the node it hangs from.
    Pending {
        /// The node the variation will hang from.
        after: NodeId,
    },
}

impl Inline {
    /// The text of the item as plain text (comments as they are, NAGs
    /// symbolic or `$n` as asked).
    fn push_text(&self, symbolic_nags: bool, out: &mut String) {
        match self {
            Inline::Number(text) | Inline::Comment { text, .. } => out.push_str(text),
            Inline::Move { san, .. } => out.push_str(san),
            Inline::Nag { nag, .. } => out.push_str(&nag_text(*nag, symbolic_nags)),
            Inline::Open => out.push('('),
            Inline::Close => out.push(')'),
            Inline::Space => out.push(' '),
            Inline::Pending { .. } => out.push_str("( ... )"),
        }
    }

    /// The node a click on the item goes to, if it is clickable: moves,
    /// NAGs and comments.
    pub fn node(&self) -> Option<NodeId> {
        match self {
            Inline::Move { node, .. } | Inline::Nag { node, .. } => Some(*node),
            Inline::Comment { target, .. } => match target {
                CommentTarget::After(node) | CommentTarget::Before(node) => Some(*node),
                CommentTarget::Initial => None,
            },
            _ => None,
        }
    }
}

/// A paragraph of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The identity of the paragraph across rebuilds.
    pub key: BlockKey,
    /// The revision the content last changed in. The front-end shapes a
    /// block again only when this (or the width) changed.
    pub rev: u64,
    /// Indentation level, `0..=`[`MAX_INDENT`].
    pub indent: u8,
    /// What the block holds.
    pub kind: BlockKind,
    /// The items.
    pub inlines: Vec<Inline>,
}

/// The game as the notation panel shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotationDoc {
    /// Grows on every rebuild that changed something (see
    /// [`NotationDoc::keep_revisions`]).
    pub revision: u64,
    /// The header lines, per [`NotationOptions::header`].
    pub header: Vec<HeaderLine>,
    /// The moves, comments and variations.
    pub blocks: Vec<Block>,
    /// The result, when [`NotationOptions::show_result`] is on.
    pub result: Option<GameResult>,
    /// Whether the NAGs of this document are symbols (the options it was
    /// built with); needed to turn it into plain text.
    pub symbolic_nags: bool,
}

impl Default for NotationDoc {
    fn default() -> NotationDoc {
        build_notation(&Game::new(), &NotationOptions::default(), 0)
    }
}

impl NotationDoc {
    /// Gives each block the revision of the block of `previous` that has
    /// the same key and the same content, so that only edited blocks look
    /// new. Cost: one pass over both documents.
    pub fn keep_revisions(&mut self, previous: &NotationDoc) {
        use std::collections::HashMap;
        let old: HashMap<BlockKey, &Block> = previous.blocks.iter().map(|b| (b.key, b)).collect();
        for block in &mut self.blocks {
            if let Some(&same) = old.get(&block.key)
                && same.indent == block.indent
                && same.kind == block.kind
                && same.inlines == block.inlines
            {
                block.rev = same.rev;
            }
        }
    }

    /// The movetext as one line: blocks joined by a space, then the result.
    /// With [`NotationLayout::Inline`] this is the text of the single block. It is
    /// what the PGN writer's movetext reads like, minus the braces around
    /// comments.
    pub fn flat_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            if !out.is_empty() {
                out.push(' ');
            }
            for inline in &block.inlines {
                inline.push_text(self.symbolic_nags, &mut out);
            }
        }
        if let Some(result) = self.result {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(result.as_str());
        }
        out
    }

    /// One line per block, indented two spaces per level, then the result
    /// (a rendering for tests and logs).
    pub fn to_plain(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            for _ in 0..block.indent {
                out.push_str("  ");
            }
            for inline in &block.inlines {
                inline.push_text(self.symbolic_nags, &mut out);
            }
            out.push('\n');
        }
        if let Some(result) = self.result {
            // `write!` to a `String` cannot fail.
            let _ = writeln!(out, "{}", result.as_str());
        }
        out
    }

    /// The block and item index of the move of `node`, to scroll to it.
    pub fn find_move(&self, node: NodeId) -> Option<(usize, usize)> {
        self.blocks.iter().enumerate().find_map(|(b, block)| {
            block
                .inlines
                .iter()
                .position(|inline| matches!(inline, Inline::Move { node: n, .. } if *n == node))
                .map(|i| (b, i))
        })
    }
}

/// The text of a NAG: its symbol (`!`, `+-`, `⊙`...) when `symbolic`, else
/// `$n`. NAGs without a symbol are `$n` either way.
pub fn nag_text(nag: Nag, symbolic: bool) -> Cow<'static, str> {
    if symbolic {
        nag.glyph()
    } else {
        Cow::Owned(nag.to_string())
    }
}

/// Builds the document for `game`.
///
/// `revision` stamps the document and every block; call
/// [`NotationDoc::keep_revisions`] to keep older revisions on unchanged
/// blocks. Walks the tree with an explicit stack, so a very long line or
/// very deep variations cannot overflow the call stack.
pub fn build_notation(game: &Game, opts: &NotationOptions, revision: u64) -> NotationDoc {
    build::build(game, opts, revision, None)
}

/// Like [`build_notation`], plus the placeholder of a variation that was
/// started at `pending` and has no move yet (D17 of the resolved
/// questions).
pub fn build_notation_with_pending(
    game: &Game,
    opts: &NotationOptions,
    revision: u64,
    pending: Option<NodeId>,
) -> NotationDoc {
    build::build(game, opts, revision, pending)
}
