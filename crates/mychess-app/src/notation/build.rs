//! The walk that turns the game tree into blocks.
//!
//! A port of the movetext writer of `mychess-pgn` (itself a port of Scid's
//! `Game::WriteMoveList`, `src/game.cpp:3088-3530`): the tree is walked with
//! an explicit stack of [`Frame`]s, one per line being written, so neither a
//! long main line nor deeply nested variations can overflow the call stack.
//! Numbering follows the same rules as the writer, so the flat text of the
//! document and the exported movetext agree.

use std::borrow::Cow;

use mychess_core::{Color, Game, GameError, NodeId, strip_mark_codes};

use super::header::header_lines;
use super::{
    Block, BlockKey, BlockKind, CommentTarget, Inline, MAX_INDENT, NotationDoc, NotationLayout,
    NotationOptions, nag_text,
};

/// One line (main line or variation) being written.
#[derive(Debug)]
struct Frame {
    /// Node whose position precedes `cur` (the parent of `cur`).
    prev: NodeId,
    /// Next node of this line.
    cur: Option<NodeId>,
    /// `cur` has not been written yet and heads a variation.
    head: bool,
    /// Alternatives still to write after the node just written: the
    /// children of this node's parent from this index on.
    vars: Option<(NodeId, usize)>,
    /// The next move needs a number even if it is Black's.
    print_num: bool,
    /// Number of enclosing variations.
    depth: u32,
    /// Where the line's items go.
    kind: BlockKind,
    /// Indentation of those blocks.
    indent: u8,
}

/// The blocks being produced, with the spacing state.
struct Out {
    blocks: Vec<Block>,
    /// The block being filled; opened by the first item pushed.
    open: Option<Block>,
    kind: BlockKind,
    indent: u8,
    /// A blank is due before the next item (unless it starts a block or
    /// follows `(`).
    space: bool,
    revision: u64,
}

impl Out {
    fn new(revision: u64) -> Out {
        Out {
            blocks: Vec::new(),
            open: None,
            kind: BlockKind::Main,
            indent: 0,
            space: false,
            revision,
        }
    }

    /// Closes the block being filled.
    fn flush(&mut self) {
        if let Some(block) = self.open.take() {
            self.blocks.push(block);
        }
    }

    /// Closes the open block and sets where the next items go.
    fn set_context(&mut self, kind: BlockKind, indent: u8) {
        self.flush();
        self.kind = kind;
        self.indent = indent;
        self.space = false;
    }

    fn new_block(&self, kind: BlockKind, indent: u8, node: Option<NodeId>) -> Block {
        Block {
            key: BlockKey { kind, node },
            rev: self.revision,
            indent,
            kind,
            inlines: Vec::new(),
        }
    }

    /// Appends an item to the open block (opening one if needed), preceded
    /// by a blank if one is due.
    fn push(&mut self, inline: Inline) {
        let mut block = match self.open.take() {
            Some(block) => block,
            None => self.new_block(self.kind, self.indent, None),
        };
        if self.space {
            if block
                .inlines
                .last()
                .is_some_and(|last| *last != Inline::Open)
            {
                block.inlines.push(Inline::Space);
            }
            self.space = false;
        }
        if block.key.node.is_none()
            && block.kind != BlockKind::Comment
            && let Inline::Move { node, .. } = &inline
        {
            block.key.node = Some(*node);
        }
        block.inlines.push(inline);
        self.open = Some(block);
    }

    /// Writes one item as a block of its own, between the blocks of the
    /// line.
    fn push_block(&mut self, kind: BlockKind, indent: u8, node: Option<NodeId>, inline: Inline) {
        self.flush();
        let mut block = self.new_block(kind, indent, node);
        block.inlines.push(inline);
        self.blocks.push(block);
        self.space = false;
    }

    /// Writes `)` at the end of the last item, even if that is in a block
    /// that was already closed (a variation ending in a sub-variation).
    fn close_paren(&mut self) {
        if let Some(block) = self.open.as_mut().filter(|b| !b.inlines.is_empty()) {
            block.inlines.push(Inline::Close);
        } else if let Some(block) = self.blocks.last_mut() {
            block.inlines.push(Inline::Close);
        } else {
            self.push(Inline::Close);
        }
        self.space = false;
    }
}

struct Walker<'a> {
    game: &'a Game,
    opts: &'a NotationOptions,
    out: Out,
    pending: Option<NodeId>,
}

pub(super) fn build(
    game: &Game,
    opts: &NotationOptions,
    revision: u64,
    pending: Option<NodeId>,
) -> NotationDoc {
    let mut walker = Walker {
        game,
        opts,
        out: Out::new(revision),
        pending,
    };
    // The nodes come from `game` itself, so the lookups cannot fail; if one
    // ever did, the document would just stop there.
    drop(walker.run());
    walker.out.flush();
    NotationDoc {
        revision,
        header: header_lines(game, opts.header),
        blocks: walker.out.blocks,
        result: opts.show_result.then_some(game.tags().result),
        symbolic_nags: opts.symbolic_nags,
    }
}

impl Walker<'_> {
    /// Whether variations get blocks of their own (`Table` builds the same
    /// blocks as `Indented`, M2-5 D241).
    fn indented(&self) -> bool {
        matches!(
            self.opts.layout,
            NotationLayout::Indented | NotationLayout::Table
        )
    }

    /// Main-line comments on blocks of their own; always so in `Table`.
    fn indent_comments(&self) -> bool {
        self.opts.indent_comments || self.opts.layout == NotationLayout::Table
    }

    /// `12. e4` instead of `12.e4`; never in `Table`.
    fn number_space(&self) -> bool {
        self.opts.move_number_space && self.opts.layout != NotationLayout::Table
    }

    /// Applies the comment options: marks removed, ends trimmed, `None`
    /// when nothing is left (the Scid's `CommentEmpty`).
    fn clean(&self, text: &str) -> Option<Box<str>> {
        let stripped = if self.opts.strip_marks {
            strip_mark_codes(text)
        } else {
            Cow::Borrowed(text)
        };
        let trimmed = stripped.trim();
        (!trimmed.is_empty()).then(|| trimmed.into())
    }

    fn run(&mut self) -> Result<(), GameError> {
        let game = self.game;
        let root = game.root();

        if self.opts.show_comments
            && let Some(text) = game.initial_comment().and_then(|c| self.clean(c))
        {
            let inline = Inline::Comment {
                target: CommentTarget::Initial,
                text,
            };
            if self.indented() {
                let indent = u8::from(self.indent_comments());
                self.out
                    .push_block(BlockKind::Comment, indent, Some(root), inline);
            } else {
                self.out.push(inline);
                self.out.space = true;
            }
        }

        let mut stack = vec![Frame {
            prev: root,
            cur: game.children(root)?.first().copied(),
            head: false,
            vars: None,
            print_num: true,
            depth: 0,
            kind: BlockKind::Main,
            indent: 0,
        }];

        while let Some(top) = stack.len().checked_sub(1) {
            if let Some((parent, index)) = stack[top].vars {
                if let Some(&variation) = game.children(parent)?.get(index) {
                    stack[top].vars = Some((parent, index + 1));
                    stack[top].print_num = true;
                    let child = self.open_variation(&stack[top], variation)?;
                    stack.push(child);
                } else {
                    stack[top].vars = None;
                    if self.pending == Some(parent) {
                        self.pending_marker(&stack[top]);
                        stack[top].print_num = true;
                    }
                }
                continue;
            }
            if let Some(node) = stack[top].cur {
                self.write_node(&mut stack[top], node)?;
                continue;
            }
            if let Some(frame) = stack.pop() {
                self.close_line(&frame);
                if let Some(outer) = stack.last() {
                    self.restore(outer);
                }
            }
        }
        Ok(())
    }

    /// Puts the output back where the line `frame` writes after a nested
    /// variation ended.
    fn restore(&mut self, frame: &Frame) {
        if self.indented() {
            self.out.set_context(frame.kind, frame.indent);
        }
    }

    /// Where a variation nested `depth` levels deep goes.
    fn variation_indent(depth: u32) -> u8 {
        u8::try_from(depth).unwrap_or(MAX_INDENT).min(MAX_INDENT)
    }

    /// Opens the variation starting at `variation`, an alternative to the
    /// main continuation of its parent.
    fn open_variation(&mut self, outer: &Frame, variation: NodeId) -> Result<Frame, GameError> {
        let depth = outer.depth.saturating_add(1);
        let (kind, indent) = if self.indented() {
            let indent = Self::variation_indent(depth);
            self.out.set_context(BlockKind::Variation, indent);
            (BlockKind::Variation, indent)
        } else {
            self.out.space = true;
            (BlockKind::Main, 0)
        };
        self.out.push(Inline::Open);
        if self.opts.show_comments
            && let Some(text) = self
                .game
                .comment_before(variation)?
                .and_then(|c| self.clean(c))
        {
            self.out.push(Inline::Comment {
                target: CommentTarget::Before(variation),
                text,
            });
            self.out.space = true;
        }
        Ok(Frame {
            prev: self.game.parent(variation)?.unwrap_or(outer.prev),
            cur: Some(variation),
            head: true,
            vars: None,
            print_num: true,
            depth,
            kind,
            indent,
        })
    }

    /// The `( ... )` of a variation that has no move yet.
    fn pending_marker(&mut self, frame: &Frame) {
        let Some(after) = self.pending else {
            return;
        };
        let inline = Inline::Pending { after };
        if self.indented() {
            let indent = Self::variation_indent(frame.depth.saturating_add(1));
            self.out
                .push_block(BlockKind::Variation, indent, None, inline);
        } else {
            self.out.space = true;
            self.out.push(inline);
        }
    }

    fn close_line(&mut self, frame: &Frame) {
        if frame.depth > 0 {
            self.out.close_paren();
        }
    }

    /// Writes the move `node` with its NAGs and comment, then queues its
    /// alternatives and advances the line.
    fn write_node(&mut self, frame: &mut Frame, node: NodeId) -> Result<(), GameError> {
        let game = self.game;
        let opts = self.opts;
        let before = game.position_at(frame.prev)?;
        let mv = game.mv(node)?;
        let white = before.turn() == Color::White;
        let fullmove = before.fullmove_number();
        let is_main = !frame.head;
        frame.head = false;

        let san: Box<str> = if mv.is_null() {
            "--".into()
        } else {
            // The move was legal when it was added; "?" only guards a
            // corrupted tree.
            before
                .san(mv)
                .map_or_else(|_| "?".into(), String::into_boxed_str)
        };

        let need_number = white || frame.print_num;
        frame.print_num = false;
        self.out.space = true;
        if need_number {
            let space = self.number_space();
            let number = match (white, space) {
                (true, _) => format!("{fullmove}."),
                (false, true) => format!("{fullmove}. ..."),
                (false, false) => format!("{fullmove}..."),
            };
            self.out.push(Inline::Number(number.into()));
            if space {
                self.out.push(Inline::Space);
            }
        }
        self.out.push(Inline::Move {
            node,
            san,
            depth: u8::try_from(frame.depth).unwrap_or(u8::MAX),
            in_variation: frame.depth > 0,
        });

        if opts.show_comments {
            for (i, &nag) in game.nags(node)?.iter().enumerate() {
                let sticks = i == 0 && nag_text(nag, opts.symbolic_nags).starts_with(['!', '?']);
                self.out.space = !sticks;
                self.out.push(Inline::Nag { node, nag });
            }
            if let Some(text) = game.comment(node)?.and_then(|c| self.clean(c)) {
                let inline = Inline::Comment {
                    target: CommentTarget::After(node),
                    text,
                };
                if self.indented() && self.indent_comments() && frame.depth == 0 {
                    self.out
                        .push_block(BlockKind::Comment, 1, Some(node), inline);
                } else {
                    self.out.space = true;
                    self.out.push(inline);
                }
                frame.print_num = true;
            }
        }

        if opts.show_variations
            && is_main
            && (game.children(frame.prev)?.len() > 1 || self.pending == Some(frame.prev))
        {
            frame.vars = Some((frame.prev, 1));
        }
        frame.prev = node;
        frame.cur = game.children(node)?.first().copied();
        Ok(())
    }
}
