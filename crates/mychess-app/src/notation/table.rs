//! The notation as a table: one row per full move (M2-5, D241-D254).
//!
//! [`NotationTable::from_doc`] is a *projection* of a [`NotationDoc`] built
//! with [`NotationLayout::Table`](super::NotationLayout::Table): the blocks
//! are read in order and turned into rows, so the document, its plain-text
//! oracle and the PGN writer oracle are untouched. The number and the colour
//! of each move come from the **game tree** (the position before the move),
//! never from the text of the [`Inline::Number`] items.
//!
//! The projection is linear over the blocks (no recursion), so deep trees
//! cannot overflow the stack. Moving the cursor never rebuilds the table:
//! it is cached with the document (see `state/notation_cache.rs`).

use std::collections::HashMap;

use mychess_core::{Color, Game, GameResult, Nag, NodeId, clock_of, emt_of};

use super::{BlockKind, Inline, NotationDoc, NotationOptions};

/// Deepest variation card indentation (D248; the document goes to
/// [`MAX_INDENT`](super::MAX_INDENT)).
pub const MAX_CARD_INDENT: u8 = 4;

/// What a row holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowKind {
    /// A full move: `number | white | black`.
    Moves,
    /// A comment on a row of its own.
    Comment,
    /// A variation card.
    Variation,
    /// The placeholder of a variation that was started and has no move yet.
    Pending,
    /// The result at the end.
    Result,
}

/// A stable identity for a row across rebuilds: the kind and the node of the
/// first move (or of the comment or card).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowKey {
    /// The kind of row.
    pub kind: RowKind,
    /// The first move of the row; the node a comment belongs to; the node a
    /// pending variation hangs from; `None` for the result.
    pub node: Option<NodeId>,
}

/// The times of one move (D250). Seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveTimes {
    /// Time spent on the move: its `[%emt]`, or (when missing, on the main
    /// line only) the clock of the previous move of the same side minus this
    /// clock, if not negative.
    pub spent: Option<u32>,
    /// The clock after the move (`[%clk]`).
    pub clock: Option<u32>,
    /// `spent` is the difference of two clocks, so it ignores the increment:
    /// an estimate.
    pub spent_is_derived: bool,
}

/// One move of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The node the move leads to.
    pub node: NodeId,
    /// The move in English SAN (`--` for the null move).
    pub san: Box<str>,
    /// The NAGs of the move, in the document order (empty with comments off).
    pub nags: Vec<Nag>,
    /// The times of the move.
    pub times: MoveTimes,
}

/// The content of a [`Row`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowContent {
    /// A full move. `white` is `None` when the row starts with Black (after
    /// a comment or a variation, or a game started with Black to move): the
    /// front-end draws `…` there. `black` is `None` while White's move has no
    /// answer or when a comment or variation follows it.
    Moves {
        /// The move number.
        number: u32,
        /// White's move.
        white: Option<Cell>,
        /// Black's move.
        black: Option<Cell>,
    },
    /// A comment; the index of the [`Block`](super::Block) of the document.
    Comment {
        /// Index in [`NotationDoc::blocks`].
        block: usize,
    },
    /// A variation card; the index of the block. The card is drawn without
    /// the first [`Inline::Open`] and the last [`Inline::Close`].
    Variation {
        /// Index in [`NotationDoc::blocks`].
        block: usize,
    },
    /// The `( ... )` placeholder of a variation without moves.
    Pending {
        /// The node the variation will hang from.
        after: NodeId,
    },
    /// The result of the game.
    Result(GameResult),
}

/// A row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The identity of the row across rebuilds.
    pub key: RowKey,
    /// The revision the content last changed in; the front-end shapes the
    /// row again only when this (or the width) changed (D254).
    pub rev: u64,
    /// Indentation of comments and cards (cards up to
    /// [`MAX_CARD_INDENT`]); 0 for move rows.
    pub indent: u8,
    /// What the row holds.
    pub content: RowContent,
}

/// Where a move is in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePlace {
    /// In a cell of a move row.
    Cell {
        /// Index in [`NotationTable::rows`].
        row: usize,
        /// The column: [`Color::White`] or [`Color::Black`].
        side: Color,
    },
    /// In a variation card.
    Card {
        /// Index in [`NotationTable::rows`] of the card.
        row: usize,
        /// Index of the [`Inline::Move`] in the block's inlines.
        inline: usize,
    },
}

/// The game as the table of the notation panel (D242).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotationTable {
    /// The revision of the document it was projected from.
    pub revision: u64,
    /// The rows, top to bottom.
    pub rows: Vec<Row>,
    /// Some move has a time (and the time column is on): the front-end draws
    /// the time column only then (D250).
    pub has_times: bool,
}

struct Builder<'a> {
    game: &'a Game,
    rows: Vec<Row>,
    /// The index of the row whose Black cell can still be filled.
    open: Option<usize>,
    /// The cell the NAGs that follow belong to.
    last: Option<(usize, Color)>,
    with_times: bool,
    has_times: bool,
}

impl NotationTable {
    /// Projects `doc` (built from `game` with
    /// [`NotationLayout::Table`](super::NotationLayout::Table)) into rows.
    ///
    /// The times are computed only when `opts.show_time_column` is on. They
    /// come from the game, so they are present even with comments hidden.
    /// A document of another layout (variations inside a main block) does
    /// not panic, but its variation moves land in the cells of the main line.
    pub fn from_doc(doc: &NotationDoc, game: &Game, opts: &NotationOptions) -> NotationTable {
        let mut builder = Builder {
            game,
            rows: Vec::with_capacity(doc.blocks.len() + 8),
            open: None,
            last: None,
            with_times: opts.show_time_column,
            has_times: false,
        };
        for (index, block) in doc.blocks.iter().enumerate() {
            match block.kind {
                BlockKind::Main => {
                    for inline in &block.inlines {
                        match inline {
                            Inline::Move {
                                node,
                                san,
                                in_variation,
                                ..
                            } => builder.push_move(*node, san, *in_variation, block.rev),
                            Inline::Nag { node, nag } => builder.push_nag(*node, *nag),
                            _ => {}
                        }
                    }
                }
                BlockKind::Comment => {
                    builder.close();
                    builder.rows.push(Row {
                        key: RowKey {
                            kind: RowKind::Comment,
                            node: block.key.node,
                        },
                        rev: block.rev,
                        indent: block.indent,
                        content: RowContent::Comment { block: index },
                    });
                }
                BlockKind::Variation => {
                    builder.close();
                    let indent = block.indent.min(MAX_CARD_INDENT);
                    let pending = block.inlines.iter().find_map(|inline| match inline {
                        Inline::Pending { after } => Some(*after),
                        _ => None,
                    });
                    let has_move = block
                        .inlines
                        .iter()
                        .any(|inline| matches!(inline, Inline::Move { .. }));
                    let (kind, node, content) = match pending {
                        Some(after) if !has_move => {
                            (RowKind::Pending, Some(after), RowContent::Pending { after })
                        }
                        _ => (
                            RowKind::Variation,
                            block.key.node,
                            RowContent::Variation { block: index },
                        ),
                    };
                    builder.rows.push(Row {
                        key: RowKey { kind, node },
                        rev: block.rev,
                        indent,
                        content,
                    });
                }
            }
        }
        builder.close();
        if let Some(result) = doc.result {
            builder.rows.push(Row {
                key: RowKey {
                    kind: RowKind::Result,
                    node: None,
                },
                rev: doc.revision,
                indent: 0,
                content: RowContent::Result(result),
            });
        }
        NotationTable {
            revision: doc.revision,
            rows: builder.rows,
            has_times: builder.has_times,
        }
    }

    /// Gives each move or result row the revision of the row of `previous`
    /// that has the same key and the same content, so that editing a NAG
    /// makes only its row look new (D254). Comment and card rows already
    /// carry the revision of their block, which
    /// [`NotationDoc::keep_revisions`] keeps. One pass over both tables.
    pub fn keep_revisions(&mut self, previous: &NotationTable) {
        let old: HashMap<RowKey, &Row> = previous.rows.iter().map(|r| (r.key, r)).collect();
        for row in &mut self.rows {
            if !matches!(row.key.kind, RowKind::Moves | RowKind::Result) {
                continue;
            }
            if let Some(&same) = old.get(&row.key)
                && same.indent == row.indent
                && same.content == row.content
            {
                row.rev = same.rev;
            }
        }
    }

    /// Where the move of `node` is: the row and column, or the card and the
    /// item index (to scroll to it). `doc` is the document the table was
    /// projected from.
    pub fn find_move(&self, doc: &NotationDoc, node: NodeId) -> Option<MovePlace> {
        self.rows
            .iter()
            .enumerate()
            .find_map(|(row, entry)| match &entry.content {
                RowContent::Moves { white, black, .. } => {
                    let is = |cell: &Option<Cell>| cell.as_ref().is_some_and(|c| c.node == node);
                    if is(white) {
                        Some(MovePlace::Cell {
                            row,
                            side: Color::White,
                        })
                    } else if is(black) {
                        Some(MovePlace::Cell {
                            row,
                            side: Color::Black,
                        })
                    } else {
                        None
                    }
                }
                RowContent::Variation { block } => {
                    let block = doc.blocks.get(*block)?;
                    block
                        .inlines
                        .iter()
                        .position(|i| matches!(i, Inline::Move { node: n, .. } if *n == node))
                        .map(|inline| MovePlace::Card { row, inline })
                }
                _ => None,
            })
    }
}

impl Builder<'_> {
    /// Ends the row being filled.
    fn close(&mut self) {
        self.open = None;
        self.last = None;
    }

    fn push_move(&mut self, node: NodeId, san: &str, in_variation: bool, rev: u64) {
        // The nodes come from `game` itself, so the lookups cannot fail; if
        // one ever did, the move would just be left out.
        let Ok(Some(parent)) = self.game.parent(node) else {
            return;
        };
        let Ok(before) = self.game.position_at(parent) else {
            return;
        };
        let (color, number) = (before.turn(), before.fullmove_number());
        let cell = Cell {
            node,
            san: san.into(),
            nags: Vec::new(),
            times: self.times(node, in_variation),
        };
        if color == Color::Black
            && let Some(index) = self.open.take()
            && let Some(Row {
                content:
                    RowContent::Moves {
                        number: open_number,
                        white: Some(_),
                        black,
                    },
                ..
            }) = self.rows.get_mut(index)
            && *open_number == number
            && black.is_none()
        {
            *black = Some(cell);
            self.last = Some((index, Color::Black));
            return;
        }
        let index = self.rows.len();
        let (white, black) = match color {
            Color::White => (Some(cell), None),
            Color::Black => (None, Some(cell)),
        };
        self.rows.push(Row {
            key: RowKey {
                kind: RowKind::Moves,
                node: Some(node),
            },
            rev,
            indent: 0,
            content: RowContent::Moves {
                number,
                white,
                black,
            },
        });
        self.open = (color == Color::White).then_some(index);
        self.last = Some((index, color));
    }

    fn push_nag(&mut self, node: NodeId, nag: Nag) {
        let Some((index, color)) = self.last else {
            return;
        };
        if let Some(Row {
            content: RowContent::Moves { white, black, .. },
            ..
        }) = self.rows.get_mut(index)
        {
            let slot = match color {
                Color::White => white,
                Color::Black => black,
            };
            if let Some(cell) = slot.as_mut().filter(|c| c.node == node) {
                cell.nags.push(nag);
            }
        }
    }

    /// The times of the move of `node` (D250); nothing with the column off.
    fn times(&mut self, node: NodeId, in_variation: bool) -> MoveTimes {
        if !self.with_times {
            return MoveTimes::default();
        }
        let comment = self.game.comment(node).ok().flatten().unwrap_or_default();
        let clock = clock_of(comment);
        let mut spent = emt_of(comment);
        let mut derived = false;
        if spent.is_none()
            && !in_variation
            && let Some(now) = clock
            && let Some(before) = self.clock_of_previous_same_side(node)
            && before >= now
        {
            spent = Some(before - now);
            derived = true;
        }
        if spent.is_some() || clock.is_some() {
            self.has_times = true;
        }
        MoveTimes {
            spent,
            clock,
            spent_is_derived: derived,
        }
    }

    /// The `[%clk]` of the previous move of the same side: the grandparent.
    fn clock_of_previous_same_side(&self, node: NodeId) -> Option<u32> {
        let parent = self.game.parent(node).ok()??;
        let grandparent = self.game.parent(parent).ok()??;
        if grandparent == self.game.root() {
            return None;
        }
        clock_of(self.game.comment(grandparent).ok()??)
    }
}
