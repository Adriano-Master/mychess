//! The game tree: a start position, tags and a tree of moves with
//! variations, comments and NAGs.
//!
//! # Model
//!
//! A [`Game`] is an arena of nodes addressed by [`NodeId`]. The **root**
//! stands for the start position; every other node is one move played from
//! the position of its parent. `children(n)[0]` is the main continuation and
//! `children(n)[1..]` are the variations, i.e. the alternatives to
//! `children(n)[0]`, in insertion order. The main line is `children[0]`
//! followed repeatedly. The "current node" cursor is not part of the game
//! (it belongs to the application state).
//!
//! # Guarantees
//!
//! * every non-root node holds a move that is legal in its parent's position
//!   (the move is validated by [`Game::add_move`]; no other operation can
//!   break this);
//! * the structure is a tree: nodes are only ever appended below existing
//!   nodes, so cycles are impossible;
//! * no method panics on an unknown [`NodeId`]; it returns
//!   [`GameError::InvalidNode`] instead.
//!
//! # Ids and the arena
//!
//! * The arena is a `Vec` of slots. Deleting a subtree frees its slots, which
//!   go to a free list and are **reused** by later [`Game::add_move`] calls,
//!   so the arena does not grow without bound under edit/undo cycles. A freed
//!   slot keeps no node (only a generation counter and a link), so it is
//!   small.
//! * A [`NodeId`] is `(game, index, generation)`. The generation of a slot
//!   increases every time the slot is freed, so an id of a deleted node stays
//!   invalid for ever, even after its slot is reused. The `game` part is
//!   unique per [`Game::new`]/[`Game::from_position`]/[`Game::truncate_start`]
//!   call, so an id from another game is invalid too. A [`Clone`] of a game
//!   keeps the same `game` part (see [`Game`] for why and for the pitfall).
//! * Each node stores its position, so [`Game::position_at`] is O(1) and
//!   returns a reference.
//! * The **initial comment** of the game (PGN `{text} 1. e4`) is the
//!   [`Game::comment`] of the root; [`Game::initial_comment`] is a shortcut.
//!   A root has no move, hence no `comment_before`, NAGs or parent.
//! * Nothing here is recursive in the length or nesting depth of a line:
//!   traversals, removals, cloning and dropping use explicit stacks or flat
//!   storage, so games with thousands of moves or variation levels are fine.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

use thiserror::Error;

use crate::{Move, MoveError, Nag, Position, Tags};

/// Identity of a [`Game`] (and of its clones). Process-wide counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct GameId(u32);

impl GameId {
    fn fresh() -> GameId {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        // Wraps after 2^32 games created in one process; an id collision then
        // would additionally need a matching index and generation.
        GameId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Identifies a node of a [`Game`].
///
/// An id is only valid for the game that produced it (and its clones) and for
/// as long as the node lives: ids of deleted nodes, and ids from other games,
/// are rejected with [`GameError::InvalidNode`]. The value has no meaning as
/// a number; use it only as an opaque handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    game: GameId,
    index: u32,
    generation: u32,
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.generation == 0 {
            write!(f, "#{}", self.index)
        } else {
            write!(f, "#{}.{}", self.index, self.generation)
        }
    }
}

/// Error from a [`Game`] operation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum GameError {
    /// The id does not name a live node of this game (unknown, deleted, or
    /// from another game).
    #[error("node {0} does not exist in this game")]
    InvalidNode(NodeId),
    /// The operation needs a move node but was given the root.
    #[error("the root has no move")]
    IsRoot,
    /// The operation needs a variation (a child other than `children[0]`).
    #[error("node {0} is the main continuation, not a variation")]
    NotVariation(NodeId),
    /// The move is not legal in the position of the parent node.
    #[error(transparent)]
    IllegalMove(#[from] MoveError),
    /// The game has more nodes than a [`NodeId`] can address.
    #[error("too many nodes in the game")]
    TooManyNodes,
}

#[derive(Debug, Clone)]
struct Node {
    mv: Option<Move>,
    parent: Option<NodeId>,
    ply: u32,
    position: Position,
    comment: Option<String>,
    comment_before: Option<String>,
    nags: Vec<Nag>,
    children: Vec<NodeId>,
}

/// Marks the end of the free list.
const NO_SLOT: u32 = u32::MAX;

/// One arena entry. A free slot holds no node (`node` is `None`) and links
/// to the next free slot, so it costs 16 bytes however large a [`Node`] is;
/// the node is boxed for that reason.
#[derive(Debug, Clone)]
struct Slot {
    /// Incremented each time the slot is freed.
    generation: u32,
    /// Next free slot while this one is free; unused while live.
    next_free: u32,
    node: Option<Box<Node>>,
}

/// A chess game: start position, tags and a tree of moves.
///
/// See the module documentation for the model.
///
/// # Cloning
///
/// `Clone` is a snapshot (the MVU state keeps clones for undo) and keeps the
/// **same game identity**: every [`NodeId`] valid in the original at the
/// moment of the clone is valid, and names the same node, in the clone, so a
/// cursor survives an undo. The pitfall: after the two diverge, ids created
/// by either one are not portable to the other (a slot may be reused for a
/// different node on each side, with the same id). Only use ids created
/// before the clone on both.
///
/// # Examples
///
/// ```
/// use mychess_core::{Game, Position};
///
/// let mut game = Game::new();
/// let pos = Position::new();
/// let e4 = game.add_move(game.root(), pos.parse_uci("e2e4")?)?;
/// let c5 = game.add_move(e4, game.position_at(e4)?.parse_uci("c7c5")?)?;
/// assert_eq!(game.mainline().collect::<Vec<_>>(), vec![e4, c5]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct Game {
    id: GameId,
    tags: Tags,
    /// Arena. Slot 0 is the root and is never freed.
    nodes: Vec<Slot>,
    /// Head of the free list, or [`NO_SLOT`].
    free_head: u32,
    live: usize,
}

impl Default for Game {
    fn default() -> Game {
        Game::new()
    }
}

impl Game {
    /// A game from the standard start position, with default tags.
    pub fn new() -> Game {
        Game::from_position(Position::new())
    }

    /// A game starting from `position` (a FEN game), with default tags.
    pub fn from_position(position: Position) -> Game {
        let root = Node {
            mv: None,
            parent: None,
            ply: 0,
            position,
            comment: None,
            comment_before: None,
            nags: Vec::new(),
            children: Vec::new(),
        };
        Game {
            id: GameId::fresh(),
            tags: Tags::default(),
            nodes: vec![Slot {
                generation: 0,
                next_free: NO_SLOT,
                node: Some(Box::new(root)),
            }],
            free_head: NO_SLOT,
            live: 1,
        }
    }

    /// The tags (header) of the game.
    pub fn tags(&self) -> &Tags {
        &self.tags
    }

    /// Mutable access to the tags.
    pub fn tags_mut(&mut self) -> &mut Tags {
        &mut self.tags
    }

    /// The root node, which stands for the start position.
    pub fn root(&self) -> NodeId {
        NodeId {
            game: self.id,
            index: 0,
            generation: 0,
        }
    }

    /// The rule set of the game, derived from the castling mode of the start
    /// position (a game created with [`Game::from_position`] inherits it).
    pub fn variant(&self) -> crate::Variant {
        self.start_position().variant()
    }

    /// The start position of the game (the position of the root, including
    /// its halfmove clock and move number).
    pub fn start_position(&self) -> &Position {
        &self.root_node().position
    }

    /// Whether `id` names a live node of this game.
    pub fn contains(&self, id: NodeId) -> bool {
        self.node(id).is_ok()
    }

    /// The number of live nodes, root included.
    pub fn node_count(&self) -> usize {
        self.live
    }

    fn root_node(&self) -> &Node {
        self.nodes
            .first()
            .and_then(|s| s.node.as_deref())
            .expect("invariant: the root slot is always live")
    }

    fn node(&self, id: NodeId) -> Result<&Node, GameError> {
        if id.game != self.id {
            return Err(GameError::InvalidNode(id));
        }
        self.nodes
            .get(id.index as usize)
            .filter(|s| s.generation == id.generation)
            .and_then(|s| s.node.as_deref())
            .ok_or(GameError::InvalidNode(id))
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, GameError> {
        if id.game != self.id {
            return Err(GameError::InvalidNode(id));
        }
        self.nodes
            .get_mut(id.index as usize)
            .filter(|s| s.generation == id.generation)
            .and_then(|s| s.node.as_deref_mut())
            .ok_or(GameError::InvalidNode(id))
    }

    fn move_node_mut(&mut self, id: NodeId) -> Result<&mut Node, GameError> {
        let node = self.node_mut(id)?;
        if node.parent.is_none() {
            return Err(GameError::IsRoot);
        }
        Ok(node)
    }

    /// Stores `node` in a free slot (or a new one) and returns its id.
    fn alloc(&mut self, node: Node) -> Result<NodeId, GameError> {
        let boxed = Box::new(node);
        let id = if self.free_head == NO_SLOT {
            let index = u32::try_from(self.nodes.len())
                .ok()
                .filter(|&i| i != NO_SLOT)
                .ok_or(GameError::TooManyNodes)?;
            self.nodes.push(Slot {
                generation: 0,
                next_free: NO_SLOT,
                node: Some(boxed),
            });
            NodeId {
                game: self.id,
                index,
                generation: 0,
            }
        } else {
            let index = self.free_head;
            let slot = self
                .nodes
                .get_mut(index as usize)
                .expect("invariant: the free list only holds arena indices");
            self.free_head = slot.next_free;
            slot.next_free = NO_SLOT;
            slot.node = Some(boxed);
            NodeId {
                game: self.id,
                index,
                generation: slot.generation,
            }
        };
        self.live += 1;
        Ok(id)
    }

    /// Frees the slot of `id` and returns its node. A slot whose generation
    /// would overflow is retired (never reused), so ids stay invalid for
    /// ever.
    fn free(&mut self, id: NodeId) -> Option<Node> {
        if id.game != self.id {
            return None;
        }
        let slot = self.nodes.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let node = slot.node.take()?;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            slot.next_free = self.free_head;
            self.free_head = id.index;
        }
        self.live -= 1;
        Some(*node)
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// The children of `id`: `[0]` is the main continuation, the rest are
    /// the variations (alternatives to `[0]`), in insertion order.
    pub fn children(&self, id: NodeId) -> Result<&[NodeId], GameError> {
        Ok(&self.node(id)?.children)
    }

    /// The alternatives to the main continuation after `id`
    /// (`children(id)[1..]`).
    pub fn variations(&self, id: NodeId) -> Result<&[NodeId], GameError> {
        Ok(self.node(id)?.children.get(1..).unwrap_or(&[]))
    }

    /// The parent of `id`; `None` for the root.
    pub fn parent(&self, id: NodeId) -> Result<Option<NodeId>, GameError> {
        Ok(self.node(id)?.parent)
    }

    /// The move that leads to `id`; [`GameError::IsRoot`] for the root.
    pub fn mv(&self, id: NodeId) -> Result<Move, GameError> {
        self.node(id)?.mv.ok_or(GameError::IsRoot)
    }

    /// Number of halfmoves from the start position to `id` (0 for the root).
    /// This counts moves in the game, whatever the move number of a FEN
    /// start position (use [`Game::position_at`] for that).
    pub fn ply(&self, id: NodeId) -> Result<u32, GameError> {
        Ok(self.node(id)?.ply)
    }

    /// The position after the move of `id` (the start position for the root).
    pub fn position_at(&self, id: NodeId) -> Result<&Position, GameError> {
        Ok(&self.node(id)?.position)
    }

    /// Whether `id` is a variation, i.e. a child that is not `children[0]`.
    pub fn is_variation(&self, id: NodeId) -> Result<bool, GameError> {
        let node = self.node(id)?;
        Ok(match node.parent {
            None => false,
            Some(p) => self.node(p)?.children.first() != Some(&id),
        })
    }

    /// The nodes of the main line, in order, excluding the root.
    ///
    /// ```
    /// use mychess_core::{Game, Position};
    ///
    /// let mut game = Game::new();
    /// let e4 = game.add_move(game.root(), Position::new().parse_uci("e2e4")?)?;
    /// assert_eq!(game.mainline().collect::<Vec<_>>(), vec![e4]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn mainline(&self) -> Mainline<'_> {
        Mainline {
            game: self,
            next: self.root_node().children.first().copied(),
        }
    }

    /// The main continuation below `id` (following `children[0]`),
    /// excluding `id` itself.
    pub fn mainline_from(&self, id: NodeId) -> Result<Mainline<'_>, GameError> {
        Ok(Mainline {
            game: self,
            next: self.node(id)?.children.first().copied(),
        })
    }

    /// All descendants of `id` in depth-first pre-order (a node before its
    /// children, `children[0]` first), excluding `id` itself. This is the
    /// order of PGN text. Uses an explicit stack.
    pub fn descendants(&self, id: NodeId) -> Result<Descendants<'_>, GameError> {
        let mut stack: Vec<NodeId> = self.node(id)?.children.clone();
        stack.reverse();
        Ok(Descendants { game: self, stack })
    }

    /// The ancestors of `id`, from its parent up to and including the root.
    pub fn ancestors(&self, id: NodeId) -> Result<Ancestors<'_>, GameError> {
        Ok(Ancestors {
            game: self,
            next: self.node(id)?.parent,
        })
    }

    /// The nodes from the first move down to `id` inclusive (empty for the
    /// root).
    pub fn path_to(&self, id: NodeId) -> Result<Vec<NodeId>, GameError> {
        let mut path = Vec::with_capacity(self.node(id)?.ply as usize);
        let mut cur = id;
        while let Some(parent) = self.node(cur)?.parent {
            path.push(cur);
            cur = parent;
        }
        path.reverse();
        Ok(path)
    }

    /// Structural equality: same start position (including halfmove clock and
    /// move number), tags and initial comment, and, in pre-order, the same
    /// moves, comments, comments before, NAGs (in order) and tree shape.
    /// Node ids, arena layout and clones' identity are ignored. Meant for
    /// round-trip tests (PGN, SI4).
    ///
    /// ```
    /// use mychess_core::{Game, Position};
    ///
    /// let mut a = Game::new();
    /// a.add_move(a.root(), Position::new().parse_uci("e2e4")?)?;
    /// let b = a.truncate_start(a.root())?;
    /// assert!(a.same_content(&b));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn same_content(&self, other: &Game) -> bool {
        let (ra, rb) = (self.root_node(), other.root_node());
        let (pa, pb) = (&ra.position, &rb.position);
        if pa != pb
            || self.variant() != other.variant()
            || pa.halfmove_clock() != pb.halfmove_clock()
            || pa.fullmove_number() != pb.fullmove_number()
            || self.tags != other.tags
        {
            return false;
        }
        let mut stack = vec![(self.root(), other.root())];
        while let Some((a, b)) = stack.pop() {
            let (Ok(na), Ok(nb)) = (self.node(a), other.node(b)) else {
                return false;
            };
            if na.mv != nb.mv
                || na.comment != nb.comment
                || na.comment_before != nb.comment_before
                || na.nags != nb.nags
                || na.children.len() != nb.children.len()
            {
                return false;
            }
            stack.extend(na.children.iter().copied().zip(nb.children.iter().copied()));
        }
        true
    }

    // ------------------------------------------------------------------
    // Editing the tree
    // ------------------------------------------------------------------

    /// The child of `parent` that plays `mv`, if there is one. The move is
    /// not validated.
    pub fn find_child(&self, parent: NodeId, mv: Move) -> Result<Option<NodeId>, GameError> {
        for &child in &self.node(parent)?.children {
            if self.node(child)?.mv == Some(mv) {
                return Ok(Some(child));
            }
        }
        Ok(None)
    }

    /// Adds `mv` as a child of `parent` and returns the node. Same as
    /// [`Game::add_move_new`] without the "was it created" flag; see there
    /// for the merge contract.
    ///
    /// ```
    /// use mychess_core::{Game, Position};
    ///
    /// let mut game = Game::new();
    /// let e4 = Position::new().parse_uci("e2e4")?;
    /// let a = game.add_move(game.root(), e4)?;
    /// let b = game.add_move(game.root(), e4)?; // merged, not duplicated
    /// assert_eq!(a, b);
    /// assert_eq!(game.node_count(), 2);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn add_move(&mut self, parent: NodeId, mv: Move) -> Result<NodeId, GameError> {
        self.add_move_new(parent, mv).map(|(id, _)| id)
    }

    /// Adds `mv` as a child of `parent`; the flag tells whether a node was
    /// created.
    ///
    /// **Merge contract:** the move is validated against the position of
    /// `parent`. If `parent` already has a child with the same move, that
    /// child is returned with `false` and **nothing changes** (no comment,
    /// NAG or order is touched; the caller decides what to do with data it
    /// meant to attach, e.g. a PGN reader warns about a duplicate variation).
    /// An existing child is found before legality is checked, so this never
    /// fails for it. Otherwise a new node is appended and returned with
    /// `true`: the main continuation if `parent` had no children, a variation
    /// if it had.
    pub fn add_move_new(&mut self, parent: NodeId, mv: Move) -> Result<(NodeId, bool), GameError> {
        self.add_child(parent, mv, false)
    }

    /// Like [`Game::add_move_new`], but also accepts a castling move whose
    /// right was lost (king and rook on their original squares, path free and
    /// safe), as the Scid's PGN reader keeps it. Uses
    /// [`Position::play_lenient`]; the resulting position has the right
    /// consumed as usual. Any other illegal move is still an error.
    ///
    /// The warning itself comes from
    /// [`Position::parse_san`] in [`SanMode::Tolerant`](crate::SanMode)
    /// ([`SanWarning::IllegalCastling`](crate::SanWarning)); this method
    /// does not report it again. The merge contract of `add_move_new` holds.
    ///
    /// ```
    /// use mychess_core::{Game, Position, SanMode};
    ///
    /// // White already lost the kingside right (FEN says `Q`).
    /// let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w Qkq - 0 1")?;
    /// let mut game = Game::from_position(pos.clone());
    /// let parsed = pos.parse_san("O-O", SanMode::Tolerant)?;
    /// assert!(!parsed.warnings.is_empty());
    /// assert!(game.add_move_new(game.root(), parsed.mv).is_err());
    /// let (id, created) = game.add_move_lenient(game.root(), parsed.mv)?;
    /// assert!(created);
    /// assert_eq!(game.position_at(id)?.fen(), "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn add_move_lenient(
        &mut self,
        parent: NodeId,
        mv: Move,
    ) -> Result<(NodeId, bool), GameError> {
        self.add_child(parent, mv, true)
    }

    fn add_child(
        &mut self,
        parent: NodeId,
        mv: Move,
        lenient: bool,
    ) -> Result<(NodeId, bool), GameError> {
        if let Some(existing) = self.find_child(parent, mv)? {
            return Ok((existing, false));
        }
        let pnode = self.node(parent)?;
        let position = if lenient {
            let mut next = pnode.position.clone();
            next.play_lenient(mv)?;
            next
        } else {
            pnode.position.after(mv)?
        };
        let ply = pnode.ply.saturating_add(1);
        let id = self.alloc(Node {
            mv: Some(mv),
            parent: Some(parent),
            ply,
            position,
            comment: None,
            comment_before: None,
            nags: Vec::new(),
            children: Vec::new(),
        })?;
        self.node_mut(parent)?.children.push(id);
        Ok((id, true))
    }

    /// Index of a variation in its parent's children, or an error if `id`
    /// is the root or the main continuation.
    fn variation_slot(&self, id: NodeId) -> Result<(NodeId, usize), GameError> {
        let parent = self.node(id)?.parent.ok_or(GameError::IsRoot)?;
        let index = self
            .node(parent)?
            .children
            .iter()
            .position(|&c| c == id)
            .ok_or(GameError::InvalidNode(id))?;
        if index == 0 {
            return Err(GameError::NotVariation(id));
        }
        Ok((parent, index))
    }

    /// Makes the variation `id` the main continuation. It moves to the front
    /// and the previous main continuation becomes the **first variation**;
    /// the other variations keep their relative order. With
    /// `[main, v1, v2, v3]`, promoting `v2` gives `[v2, main, v1, v3]`.
    /// Comments stay attached to their nodes.
    ///
    /// Scid: `src/game.cpp`, `Game::MainVariation`.
    ///
    /// Errors with [`GameError::NotVariation`] if `id` is already the main
    /// continuation.
    ///
    /// ```
    /// use mychess_core::{Game, Position};
    ///
    /// let mut game = Game::new();
    /// let pos = Position::new();
    /// let e4 = game.add_move(game.root(), pos.parse_uci("e2e4")?)?;
    /// let d4 = game.add_move(game.root(), pos.parse_uci("d2d4")?)?;
    /// game.promote_variation(d4)?;
    /// assert_eq!(game.children(game.root())?, &[d4, e4]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn promote_variation(&mut self, id: NodeId) -> Result<(), GameError> {
        let (parent, index) = self.variation_slot(id)?;
        let children = &mut self.node_mut(parent)?.children;
        let moved = children.remove(index);
        children.insert(0, moved);
        Ok(())
    }

    /// Moves the variation `id` to the first variation slot
    /// (`children[1]`); the main continuation does not change.
    pub fn make_first(&mut self, id: NodeId) -> Result<(), GameError> {
        let (parent, index) = self.variation_slot(id)?;
        let children = &mut self.node_mut(parent)?.children;
        let moved = children.remove(index);
        children.insert(1, moved);
        Ok(())
    }

    /// Deletes the variation `id` and its whole subtree. The main
    /// continuation cannot be deleted this way (see
    /// [`Game::truncate_from`]).
    pub fn delete_variation(&mut self, id: NodeId) -> Result<(), GameError> {
        let (parent, index) = self.variation_slot(id)?;
        self.node_mut(parent)?.children.remove(index);
        self.drop_subtree(id);
        Ok(())
    }

    /// Deletes the node `id` and its whole subtree (the move, what follows
    /// and all variations below it), but **not its siblings**. Works on a
    /// main continuation or a variation; if a main continuation is deleted
    /// and it had sibling variations, the first of them becomes the main
    /// continuation. Errors with [`GameError::IsRoot`] for the root.
    ///
    /// Compare [`Game::truncate_after`], which deletes everything below a
    /// node, siblings of the first move included.
    pub fn truncate_from(&mut self, id: NodeId) -> Result<(), GameError> {
        let parent = self.node(id)?.parent.ok_or(GameError::IsRoot)?;
        self.node_mut(parent)?.children.retain(|&c| c != id);
        self.drop_subtree(id);
        Ok(())
    }

    /// Deletes **all** children of `id` (the main continuation and every
    /// variation) with their subtrees; `id` becomes a leaf. Works on the root
    /// too, which empties the game of moves.
    ///
    /// This is what Scid's `Game::Truncate` does at the current position
    /// (the current move, the following ones and the variations are
    /// removed): to delete "this move and the rest" in the Scid sense, call
    /// it on the parent of the move. See [`Game::truncate_from`] to delete a
    /// single child while keeping its siblings.
    pub fn truncate_after(&mut self, id: NodeId) -> Result<(), GameError> {
        let children = std::mem::take(&mut self.node_mut(id)?.children);
        for child in children {
            self.drop_subtree(child);
        }
        Ok(())
    }

    /// Frees `id` and everything below it. The caller has already detached
    /// `id` from its parent. Iterative.
    fn drop_subtree(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            if let Some(node) = self.free(cur) {
                stack.extend(node.children);
            }
        }
    }

    /// A new game whose start position is the position at `id`, with the
    /// same tags. The continuation of `id` (main line and variations) is
    /// copied below the new root. With the root, this is a plain copy.
    ///
    /// The node `id` becomes the root of the new game, which has no move,
    /// `comment_before` or NAGs: those of `id` are **discarded**, and its
    /// comment (after the move) becomes the initial comment of the new game.
    ///
    /// The result is a **new game identity**: no [`NodeId`] of `self` is
    /// valid in it (and vice versa), unlike [`Clone`]. Map the cursor by
    /// path (`path_to`) if needed.
    pub fn truncate_start(&self, id: NodeId) -> Result<Game, GameError> {
        let start = self.node(id)?;
        let mut game = Game::from_position(start.position.clone());
        game.tags = self.tags.clone();
        if let Some(root) = game.nodes.first_mut().and_then(|s| s.node.as_deref_mut()) {
            root.comment.clone_from(&start.comment);
        }
        // Explicit stack of (old node, new parent), children pushed in
        // reverse so that they are created in order.
        let mut stack: Vec<(NodeId, NodeId)> = start
            .children
            .iter()
            .rev()
            .map(|&c| (c, game.root()))
            .collect();
        while let Some((old_id, new_parent)) = stack.pop() {
            let old = self.node(old_id)?;
            let ply = game.node(new_parent)?.ply.saturating_add(1);
            let new_id = game.alloc(Node {
                mv: old.mv,
                parent: Some(new_parent),
                ply,
                position: old.position.clone(),
                comment: old.comment.clone(),
                comment_before: old.comment_before.clone(),
                nags: old.nags.clone(),
                children: Vec::new(),
            })?;
            game.node_mut(new_parent)?.children.push(new_id);
            stack.extend(old.children.iter().rev().map(|&c| (c, new_id)));
        }
        Ok(game)
    }

    /// Removes every variation at every level, keeping the main line with
    /// its comments and NAGs.
    ///
    /// ```
    /// use mychess_core::{Game, Position};
    ///
    /// let mut game = Game::new();
    /// let pos = Position::new();
    /// game.add_move(game.root(), pos.parse_uci("e2e4")?)?;
    /// game.add_move(game.root(), pos.parse_uci("d2d4")?)?;
    /// game.strip_variations();
    /// assert_eq!(game.node_count(), 2);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn strip_variations(&mut self) {
        let mut dropped = Vec::new();
        for node in self.nodes.iter_mut().filter_map(|s| s.node.as_deref_mut()) {
            if node.children.len() > 1 {
                dropped.extend(node.children.drain(1..));
            }
        }
        for id in dropped {
            self.drop_subtree(id);
        }
    }

    /// Removes all comments (also the ones before moves and the initial
    /// comment). NAGs are kept.
    pub fn strip_comments(&mut self) {
        for node in self.nodes.iter_mut().filter_map(|s| s.node.as_deref_mut()) {
            node.comment = None;
            node.comment_before = None;
        }
    }

    /// Removes all NAGs. Comments are kept.
    pub fn strip_nags(&mut self) {
        for node in self.nodes.iter_mut().filter_map(|s| s.node.as_deref_mut()) {
            node.nags.clear();
        }
    }

    // ------------------------------------------------------------------
    // Comments
    // ------------------------------------------------------------------

    /// The comment after the move of `id`; for the root, the initial
    /// comment of the game.
    pub fn comment(&self, id: NodeId) -> Result<Option<&str>, GameError> {
        Ok(self.node(id)?.comment.as_deref())
    }

    /// Sets the comment after the move of `id` (the initial comment for the
    /// root). Empty or whitespace-only text removes the comment.
    pub fn set_comment(&mut self, id: NodeId, text: &str) -> Result<(), GameError> {
        self.node_mut(id)?.comment = non_blank(text);
        Ok(())
    }

    /// Appends `text` to the comment of `id`, adding a space in between when
    /// the existing text does not end in whitespace. Blank `text` changes
    /// nothing.
    pub fn append_comment(&mut self, id: NodeId, text: &str) -> Result<(), GameError> {
        let node = self.node_mut(id)?;
        append_text(&mut node.comment, text);
        Ok(())
    }

    /// The comment before the move of `id` (Scid's `START_MARKER` comment,
    /// `( {text} 1... e5)`). The root has none.
    pub fn comment_before(&self, id: NodeId) -> Result<Option<&str>, GameError> {
        Ok(self.node(id)?.comment_before.as_deref())
    }

    /// Sets the comment before the move of `id`. Blank text removes it.
    /// Errors with [`GameError::IsRoot`] for the root (use
    /// [`Game::set_comment`] for the initial comment).
    pub fn set_comment_before(&mut self, id: NodeId, text: &str) -> Result<(), GameError> {
        self.move_node_mut(id)?.comment_before = non_blank(text);
        Ok(())
    }

    /// The initial comment of the game (comment of the root).
    pub fn initial_comment(&self) -> Option<&str> {
        self.root_node().comment.as_deref()
    }

    /// Sets the initial comment of the game. Blank text removes it.
    pub fn set_initial_comment(&mut self, text: &str) {
        if let Some(root) = self.nodes.first_mut().and_then(|s| s.node.as_deref_mut()) {
            root.comment = non_blank(text);
        }
    }

    // ------------------------------------------------------------------
    // NAGs
    // ------------------------------------------------------------------

    /// The NAGs of the move of `id`, in insertion order (empty for the root).
    pub fn nags(&self, id: NodeId) -> Result<&[Nag], GameError> {
        Ok(&self.node(id)?.nags)
    }

    /// Adds `nag` to the move of `id`. Returns `false` (and changes nothing)
    /// if it was already present.
    pub fn add_nag(&mut self, id: NodeId, nag: Nag) -> Result<bool, GameError> {
        let node = self.move_node_mut(id)?;
        if node.nags.contains(&nag) {
            return Ok(false);
        }
        node.nags.push(nag);
        Ok(true)
    }

    /// Removes `nag` from the move of `id`. Returns whether it was present.
    pub fn remove_nag(&mut self, id: NodeId, nag: Nag) -> Result<bool, GameError> {
        let node = self.move_node_mut(id)?;
        let before = node.nags.len();
        node.nags.retain(|&n| n != nag);
        Ok(node.nags.len() != before)
    }

    /// Removes all NAGs of the move of `id`.
    pub fn clear_nags(&mut self, id: NodeId) -> Result<(), GameError> {
        self.move_node_mut(id)?.nags.clear();
        Ok(())
    }
}

fn non_blank(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

fn append_text(slot: &mut Option<String>, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    match slot {
        Some(existing) => {
            if !existing.ends_with(char::is_whitespace) {
                existing.push(' ');
            }
            existing.push_str(text);
        }
        None => *slot = Some(text.to_owned()),
    }
}

/// Iterator over a chain of main continuations, see [`Game::mainline`].
#[derive(Debug, Clone)]
pub struct Mainline<'a> {
    game: &'a Game,
    next: Option<NodeId>,
}

impl Iterator for Mainline<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self
            .game
            .node(current)
            .ok()
            .and_then(|n| n.children.first().copied());
        Some(current)
    }
}

/// Depth-first pre-order iterator, see [`Game::descendants`].
#[derive(Debug, Clone)]
pub struct Descendants<'a> {
    game: &'a Game,
    stack: Vec<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.stack.pop()?;
        if let Ok(node) = self.game.node(current) {
            self.stack.extend(node.children.iter().rev().copied());
        }
        Some(current)
    }
}

/// Iterator towards the root, see [`Game::ancestors`].
#[derive(Debug, Clone)]
pub struct Ancestors<'a> {
    game: &'a Game,
    next: Option<NodeId>,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.game.node(current).ok().and_then(|n| n.parent);
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays UCI `text` below `parent`.
    fn play(g: &mut Game, parent: NodeId, text: &str) -> NodeId {
        let pos = g.position_at(parent).expect("node").clone();
        let mv = pos.parse_uci(text).expect("uci");
        g.add_move(parent, mv).expect("add")
    }

    fn play_root(g: &mut Game, text: &str) -> NodeId {
        let root = g.root();
        play(g, root, text)
    }

    fn nag(n: u16) -> Nag {
        Nag::new(n).expect("nag")
    }

    /// An id of `g` that was never allocated.
    fn fake(g: &Game, index: u32) -> NodeId {
        NodeId {
            game: g.id,
            index,
            generation: 0,
        }
    }

    /// Checks the invariants over every live node.
    fn check_invariants(g: &Game) {
        let mut seen = 0;
        let root = g.root();
        for id in std::iter::once(root).chain(g.descendants(root).expect("root")) {
            seen += 1;
            let pos = g.position_at(id).expect("pos");
            match g.parent(id).expect("parent") {
                None => assert_eq!(id, root),
                Some(p) => {
                    let mv = g.mv(id).expect("mv");
                    assert_eq!(&g.position_at(p).expect("p").after(mv).expect("legal"), pos);
                    assert!(g.children(p).expect("c").contains(&id));
                    assert_eq!(g.ply(id).expect("ply"), g.ply(p).expect("ply") + 1);
                }
            }
            let kids = g.children(id).expect("children");
            let mut unique = kids.to_vec();
            unique.sort();
            unique.dedup();
            assert_eq!(
                unique.len(),
                kids.len(),
                "duplicate ids in children of {id}"
            );
            for &c in kids {
                assert!(g.contains(c), "dead child {c} under {id}");
                assert_eq!(g.parent(c), Ok(Some(id)), "parent of {c}");
            }
        }
        assert_eq!(seen, g.node_count());
        // Arena: live slots are exactly the reachable nodes, and the free
        // list holds only free slots, each once.
        let live = g.nodes.iter().filter(|s| s.node.is_some()).count();
        assert_eq!(live, g.node_count());
        let mut free = std::collections::HashSet::new();
        let mut cur = g.free_head;
        while cur != NO_SLOT {
            let slot = &g.nodes[cur as usize];
            assert!(slot.node.is_none(), "live slot {cur} in the free list");
            assert!(free.insert(cur), "slot {cur} twice in the free list");
            cur = slot.next_free;
        }
        for &f in &free {
            let reachable = std::iter::once(root)
                .chain(g.descendants(root).expect("root"))
                .any(|id| id.index == f);
            assert!(!reachable, "free slot {f} reachable from the root");
        }
    }

    #[test]
    fn new_game_is_just_a_root() {
        let g = Game::new();
        assert_eq!(g.node_count(), 1);
        assert_eq!(g.mainline().count(), 0);
        assert_eq!(g.parent(g.root()), Ok(None));
        assert_eq!(g.mv(g.root()), Err(GameError::IsRoot));
        assert_eq!(g.ply(g.root()), Ok(0));
        assert_eq!(g.start_position(), &Position::new());
    }

    #[test]
    fn add_move_builds_main_line_and_variations() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e2e4");
        let e5 = play(&mut g, e4, "e7e5");
        let c5 = play(&mut g, e4, "c7c5");
        let nf3 = play(&mut g, e5, "g1f3");
        assert_eq!(g.children(e4), Ok(&[e5, c5][..]));
        assert_eq!(g.variations(e4), Ok(&[c5][..]));
        assert_eq!(g.is_variation(c5), Ok(true));
        assert_eq!(g.is_variation(e5), Ok(false));
        assert_eq!(g.mainline().collect::<Vec<_>>(), vec![e4, e5, nf3]);
        assert_eq!(g.ply(nf3), Ok(3));
        assert_eq!(g.path_to(c5).expect("path"), vec![e4, c5]);
        assert_eq!(
            g.ancestors(c5).expect("anc").collect::<Vec<_>>(),
            vec![e4, g.root()]
        );
        check_invariants(&g);
    }

    #[test]
    fn add_move_returns_existing_child() {
        let mut g = Game::new();
        let a = play_root(&mut g, "e2e4");
        let b = play_root(&mut g, "e2e4");
        assert_eq!(a, b);
        assert_eq!(g.node_count(), 2);
    }

    #[test]
    fn add_move_rejects_illegal_and_invalid_parent() {
        let mut g = Game::new();
        let mv = Position::new().parse_uci("e2e4").expect("uci");
        let e4 = g.add_move(g.root(), mv).expect("add");
        // e2e4 again is illegal after 1.e4.
        assert!(matches!(
            g.add_move(e4, mv),
            Err(GameError::IllegalMove(MoveError::Illegal { .. }))
        ));
        assert_eq!(
            g.add_move(fake(&g, 99), mv),
            Err(GameError::InvalidNode(fake(&g, 99)))
        );
        assert_eq!(g.node_count(), 2);
    }

    #[test]
    fn null_move_is_supported() {
        let mut g = Game::new();
        let n = g.add_move(g.root(), Move::Null).expect("null");
        assert_eq!(g.position_at(n).expect("pos").turn(), crate::Color::Black);
    }

    #[test]
    fn fen_start_keeps_ply_relative_and_move_number() {
        let pos = Position::from_fen("8/8/8/8/8/5k2/8/R3K3 b - - 0 40").expect("fen");
        let mut g = Game::from_position(pos);
        let n = play_root(&mut g, "f3e3");
        assert_eq!(g.ply(n), Ok(1));
        assert_eq!(g.position_at(n).expect("p").fullmove_number(), 41);
        assert_eq!(g.start_position().fullmove_number(), 40);
    }

    #[test]
    fn deep_game_does_not_overflow_the_stack() {
        // Shuffle knights: 1.Nf3 Nf6 2.Ng1 Ng8 ... for 20000 plies.
        let mut g = Game::new();
        let mut cur = g.root();
        for _ in 0..5000 {
            for m in ["g1f3", "g8f6", "f3g1", "f6g8"] {
                cur = play(&mut g, cur, m);
            }
        }
        assert_eq!(g.mainline().count(), 20000);
        assert_eq!(g.descendants(g.root()).expect("d").count(), 20000);
        assert_eq!(g.path_to(cur).expect("p").len(), 20000);
        let copy = g.clone();
        g.truncate_from(g.mainline().next().expect("first"))
            .expect("t");
        assert_eq!(g.node_count(), 1);
        drop(copy);
    }

    /// 1.e4 e5 (1...c5 2.Nf3 (2.c3) 2...d6) (1...e6) 2.Nf3
    fn sample() -> (Game, [NodeId; 7]) {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e2e4");
        let e5 = play(&mut g, e4, "e7e5");
        let c5 = play(&mut g, e4, "c7c5");
        let nf3v = play(&mut g, c5, "g1f3");
        let c3 = play(&mut g, c5, "c2c3");
        let d6 = play(&mut g, nf3v, "d7d6");
        let e6 = play(&mut g, e4, "e7e6");
        (g, [e4, e5, c5, nf3v, c3, d6, e6])
    }

    #[test]
    fn descendants_is_preorder() {
        let (g, [e4, e5, c5, nf3v, c3, d6, e6]) = sample();
        let order: Vec<_> = g.descendants(g.root()).expect("d").collect();
        assert_eq!(order, vec![e4, e5, c5, nf3v, d6, c3, e6]);
    }

    #[test]
    fn promote_variation_makes_old_main_the_first_variation() {
        let (mut g, [e4, e5, c5, _, _, _, e6]) = sample();
        // [e5, c5, e6] with e6 promoted: [e6, e5, c5].
        g.promote_variation(e6).expect("promote");
        assert_eq!(g.children(e4), Ok(&[e6, e5, c5][..]));
        assert_eq!(g.promote_variation(e6), Err(GameError::NotVariation(e6)));
        assert_eq!(g.promote_variation(g.root()), Err(GameError::IsRoot));
        check_invariants(&g);
    }

    #[test]
    fn make_first_reorders_variations_only() {
        let (mut g, [e4, e5, c5, _, _, _, e6]) = sample();
        g.make_first(e6).expect("first");
        assert_eq!(g.children(e4), Ok(&[e5, e6, c5][..]));
        g.make_first(e6).expect("noop");
        assert_eq!(g.children(e4), Ok(&[e5, e6, c5][..]));
        assert_eq!(g.make_first(e5), Err(GameError::NotVariation(e5)));
    }

    #[test]
    fn delete_variation_removes_subtree_and_invalidates_ids() {
        let (mut g, [e4, e5, c5, nf3v, c3, d6, e6]) = sample();
        g.delete_variation(c5).expect("delete");
        assert_eq!(g.children(e4), Ok(&[e5, e6][..]));
        assert_eq!(g.node_count(), 4);
        for id in [c5, nf3v, c3, d6] {
            assert_eq!(g.mv(id), Err(GameError::InvalidNode(id)));
            assert!(!g.contains(id));
        }
        assert_eq!(g.delete_variation(e5), Err(GameError::NotVariation(e5)));
        assert_eq!(g.delete_variation(c5), Err(GameError::InvalidNode(c5)));
        check_invariants(&g);
    }

    #[test]
    fn delete_nested_variation_keeps_outer() {
        let (mut g, [_, _, c5, nf3v, c3, d6, _]) = sample();
        g.delete_variation(c3).expect("delete");
        assert_eq!(g.children(c5), Ok(&[nf3v][..]));
        assert!(g.contains(d6));
        check_invariants(&g);
    }

    #[test]
    fn truncate_from_main_promotes_first_sibling() {
        let (mut g, [e4, e5, c5, _, _, _, e6]) = sample();
        g.truncate_from(e5).expect("truncate");
        assert_eq!(g.children(e4), Ok(&[c5, e6][..]));
        assert_eq!(g.truncate_from(g.root()), Err(GameError::IsRoot));
        check_invariants(&g);
    }

    #[test]
    fn truncate_from_variation_removes_nested_lines() {
        let (mut g, [e4, e5, c5, ..]) = sample();
        g.truncate_from(c5).expect("truncate");
        assert_eq!(g.children(e4).map(<[NodeId]>::len), Ok(2));
        assert!(g.children(e4).expect("c").contains(&e5));
        assert_eq!(g.node_count(), 4);
        check_invariants(&g);
    }

    #[test]
    fn truncate_start_copies_continuation() {
        let (mut g, [e4, _, c5, _, _, d6, _]) = sample();
        g.tags_mut().white = "Anna".into();
        g.set_comment(e4, "after e4").expect("c");
        g.set_comment(d6, "kept").expect("c");
        g.add_nag(c5, nag(1)).expect("n");
        let t = g.truncate_start(e4).expect("start");
        assert_eq!(t.tags().white, "Anna");
        assert_eq!(t.initial_comment(), Some("after e4"));
        assert_eq!(t.start_position(), g.position_at(e4).expect("p"));
        assert_eq!(t.node_count(), 7);
        let kids = t.children(t.root()).expect("k").to_vec();
        assert_eq!(kids.len(), 3);
        assert_eq!(t.nags(kids[1]).expect("n"), &[nag(1)]);
        assert_eq!(t.ply(kids[0]), Ok(1));
        // d6 is at depth 3 below the new root
        let deepest = t
            .descendants(t.root())
            .expect("d")
            .find(|&n| t.comment(n) == Ok(Some("kept")));
        assert_eq!(deepest.map(|n| t.ply(n)), Some(Ok(3)));
        check_invariants(&t);
        // Original untouched.
        assert_eq!(g.node_count(), 8);
    }

    #[test]
    fn truncate_start_at_root_is_a_copy() {
        let (g, _) = sample();
        let t = g.truncate_start(g.root()).expect("copy");
        assert_eq!(t.node_count(), g.node_count());
        assert_eq!(
            t.descendants(t.root())
                .expect("d")
                .map(|n| t.mv(n))
                .collect::<Vec<_>>(),
            g.descendants(g.root())
                .expect("d")
                .map(|n| g.mv(n))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn strip_variations_keeps_main_line_data() {
        let (mut g, [e4, e5, _, _, _, _, _]) = sample();
        let nf3 = play(&mut g, e5, "g1f3");
        g.set_comment(nf3, "c").expect("c");
        g.add_nag(e5, nag(2)).expect("n");
        g.strip_variations();
        assert_eq!(g.mainline().collect::<Vec<_>>(), vec![e4, e5, nf3]);
        assert_eq!(g.node_count(), 4);
        assert_eq!(g.comment(nf3), Ok(Some("c")));
        assert_eq!(g.nags(e5).expect("n"), &[nag(2)]);
        check_invariants(&g);
    }

    #[test]
    fn strip_comments_and_nags_are_independent() {
        let (mut g, [e4, e5, ..]) = sample();
        g.set_initial_comment("intro");
        g.set_comment(e4, "a").expect("c");
        g.set_comment_before(e5, "b").expect("c");
        g.add_nag(e4, nag(3)).expect("n");
        g.strip_comments();
        assert_eq!(g.initial_comment(), None);
        assert_eq!(g.comment(e4), Ok(None));
        assert_eq!(g.comment_before(e5), Ok(None));
        assert_eq!(g.nags(e4).expect("n"), &[nag(3)]);
        g.set_comment(e4, "a").expect("c");
        g.strip_nags();
        assert_eq!(g.nags(e4).expect("n"), []);
        assert_eq!(g.comment(e4), Ok(Some("a")));
    }

    #[test]
    fn comments_blank_append_and_root_rules() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e2e4");
        g.set_comment(e4, "  ").expect("c");
        assert_eq!(g.comment(e4), Ok(None));
        g.append_comment(e4, "one").expect("c");
        g.append_comment(e4, "two").expect("c");
        g.append_comment(e4, "   ").expect("c");
        assert_eq!(g.comment(e4), Ok(Some("one two")));
        g.set_comment(e4, "end ").expect("c");
        g.append_comment(e4, "x").expect("c");
        assert_eq!(g.comment(e4), Ok(Some("end x")));
        g.set_comment(e4, "").expect("c");
        assert_eq!(g.comment(e4), Ok(None));
        assert_eq!(g.set_comment_before(g.root(), "x"), Err(GameError::IsRoot));
        assert_eq!(g.comment_before(g.root()), Ok(None));
        g.set_comment(g.root(), "start").expect("c");
        assert_eq!(g.initial_comment(), Some("start"));
        assert_eq!(
            g.set_comment(fake(&g, 7), "x"),
            Err(GameError::InvalidNode(fake(&g, 7)))
        );
    }

    #[test]
    fn nags_have_no_duplicates_and_keep_order() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e2e4");
        assert_eq!(g.add_nag(e4, nag(14)), Ok(true));
        assert_eq!(g.add_nag(e4, nag(1)), Ok(true));
        assert_eq!(g.add_nag(e4, nag(14)), Ok(false));
        assert_eq!(g.nags(e4).expect("n"), &[nag(14), nag(1)]);
        assert_eq!(g.remove_nag(e4, nag(14)), Ok(true));
        assert_eq!(g.remove_nag(e4, nag(14)), Ok(false));
        g.clear_nags(e4).expect("clear");
        assert_eq!(g.nags(e4).expect("n"), []);
        assert_eq!(g.add_nag(g.root(), nag(1)), Err(GameError::IsRoot));
        assert_eq!(g.nags(g.root()).expect("n"), []);
    }

    #[test]
    fn invalid_ids_never_panic() {
        let mut g = Game::new();
        let bad = fake(&g, u32::MAX);
        assert!(g.children(bad).is_err());
        assert!(g.variations(bad).is_err());
        assert!(g.parent(bad).is_err());
        assert!(g.position_at(bad).is_err());
        assert!(g.mainline_from(bad).is_err());
        assert!(g.descendants(bad).is_err());
        assert!(g.ancestors(bad).is_err());
        assert!(g.path_to(bad).is_err());
        assert!(g.truncate_start(bad).is_err());
        assert!(g.truncate_from(bad).is_err());
        assert!(g.promote_variation(bad).is_err());
        assert!(g.make_first(bad).is_err());
        assert!(g.delete_variation(bad).is_err());
        assert!(g.add_nag(bad, nag(1)).is_err());
        assert!(g.set_comment_before(bad, "x").is_err());
        assert!(g.is_variation(bad).is_err());
    }

    #[test]
    fn mainline_from_follows_first_children() {
        let (g, [e4, e5, ..]) = sample();
        let rest: Vec<_> = g.mainline_from(e4).expect("m").collect();
        assert_eq!(rest, vec![e5]);
    }

    // ------------------------------------------------------------------
    // Arena, ids, truncation, merge, nesting
    // ------------------------------------------------------------------

    /// Builds `depth` nested variation levels: at every level the first
    /// legal move is the main continuation, the second one a variation, and
    /// the next level continues inside that variation.
    fn nested(depth: usize) -> Game {
        let mut g = Game::new();
        let mut cur = g.root();
        for level in 0..depth {
            let moves = g.position_at(cur).expect("pos").legal_moves();
            assert!(moves.len() >= 2, "ran out of moves at level {level}");
            g.add_move(cur, moves[0]).expect("main");
            cur = g.add_move(cur, moves[1]).expect("variation");
        }
        g
    }

    #[test]
    fn thousands_of_nested_variation_levels_do_not_overflow_the_stack() {
        let depth = 3000;
        let g = nested(depth);
        assert_eq!(g.node_count(), 1 + 2 * depth);
        assert_eq!(g.descendants(g.root()).expect("d").count(), 2 * depth);

        let copy = g.clone();
        assert!(g.same_content(&copy));

        let mut stripped = g.clone();
        stripped.strip_variations();
        assert_eq!(stripped.node_count(), 2);
        check_invariants(&stripped);

        let t = g.truncate_start(g.root()).expect("copy");
        assert_eq!(t.node_count(), g.node_count());
        assert!(g.same_content(&t));

        let mut cut = g.clone();
        cut.truncate_after(cut.root()).expect("truncate");
        assert_eq!(cut.node_count(), 1);
        check_invariants(&cut);

        let deepest = g.descendants(g.root()).expect("d").last().expect("last");
        assert_eq!(g.ply(deepest), Ok(u32::try_from(depth).expect("fits")));
        drop(g); // flat storage: no recursive drop
        drop(copy);
    }

    #[test]
    fn freed_slots_are_reused_but_old_ids_stay_invalid() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e2e4");
        let d4 = play_root(&mut g, "d2d4");
        g.delete_variation(d4).expect("delete");
        let arena = g.nodes.len();
        let c4 = play_root(&mut g, "c2c4");
        assert_eq!(g.nodes.len(), arena, "slot reused, arena did not grow");
        assert_eq!(c4.index, d4.index);
        assert_ne!(c4.generation, d4.generation);
        assert_ne!(c4, d4);
        assert!(g.contains(c4));
        assert!(!g.contains(d4));
        assert_eq!(g.mv(d4), Err(GameError::InvalidNode(d4)));
        assert_eq!(g.delete_variation(d4), Err(GameError::InvalidNode(d4)));
        assert_eq!(g.children(g.root()), Ok(&[e4, c4][..]));
        check_invariants(&g);

        // Many edit/undo cycles keep the arena bounded.
        let n = play(&mut g, e4, "e7e5");
        g.truncate_from(n).expect("truncate");
        let arena = g.nodes.len();
        for _ in 0..100 {
            let n = play(&mut g, e4, "e7e5");
            g.truncate_from(n).expect("truncate");
        }
        assert_eq!(g.nodes.len(), arena);
        check_invariants(&g);
    }

    #[test]
    fn retired_slot_is_never_reused_when_generation_overflows() {
        let mut g = Game::new();
        let a = play_root(&mut g, "e2e4");
        // Age the slot to the last generation (and patch the child list).
        g.nodes[a.index as usize].generation = u32::MAX;
        let a = NodeId {
            generation: u32::MAX,
            ..a
        };
        let root = g.root();
        g.node_mut(root).expect("root").children = vec![a];
        g.truncate_from(a).expect("truncate");
        assert_eq!(g.free_head, NO_SLOT);
        let b = play_root(&mut g, "e2e4");
        assert_ne!(b.index, a.index);
        assert!(!g.contains(a));
        check_invariants(&g);
    }

    #[test]
    fn ids_of_another_game_are_invalid() {
        let mut a = Game::new();
        let mut b = Game::new();
        let ea = play_root(&mut a, "e2e4");
        let eb = play_root(&mut b, "e2e4");
        assert_ne!(ea, eb, "same index and generation, different game");
        assert!(!b.contains(ea));
        assert_eq!(b.children(ea), Err(GameError::InvalidNode(ea)));
        assert_eq!(b.truncate_from(ea), Err(GameError::InvalidNode(ea)));
        assert!(b.truncate_start(ea).is_err());
        assert_ne!(a.root(), b.root());
        assert!(!b.contains(a.root()));
    }

    #[test]
    fn truncate_start_makes_a_new_identity_and_clone_keeps_it() {
        let (g, [e4, e5, ..]) = sample();
        let t = g.truncate_start(e4).expect("start");
        let ids: Vec<_> = std::iter::once(t.root())
            .chain(t.descendants(t.root()).expect("d"))
            .collect();
        for id in ids {
            assert!(!g.contains(id), "id of the truncated game valid in the old");
        }
        assert!(!t.contains(e5));
        assert!(!t.contains(e4));
        // Clone: same identity, cursors survive.
        let snapshot = g.clone();
        assert!(snapshot.contains(e5));
        assert_eq!(snapshot.mv(e5), g.mv(e5));
        let copy_of_root = g.truncate_start(g.root()).expect("copy");
        assert_ne!(copy_of_root.root(), g.root());
    }

    #[test]
    fn truncate_start_keeps_clocks_and_move_number_of_the_node() {
        let pos = Position::from_fen("r3k3/8/8/8/8/8/8/R3K3 w - - 7 33").expect("fen");
        let mut g = Game::from_position(pos);
        let first = play_root(&mut g, "a1a2");
        let second = play(&mut g, first, "a8a7");
        let cut = g.truncate_start(second).expect("start");
        let (want, got) = (g.position_at(second).expect("p"), cut.start_position());
        assert_eq!(got, want);
        assert_eq!(got.halfmove_clock(), want.halfmove_clock());
        assert_eq!(got.fullmove_number(), want.fullmove_number());
        assert_eq!(got.halfmove_clock(), 9);
        assert_eq!(got.fullmove_number(), 34);
        assert_eq!(cut.ply(cut.root()), Ok(0));
        // Same for the root.
        let copy = g.truncate_start(g.root()).expect("copy");
        assert_eq!(copy.start_position().halfmove_clock(), 7);
        assert_eq!(copy.start_position().fullmove_number(), 33);
    }

    #[test]
    fn truncate_start_discards_comment_before_and_nags_of_the_start_node() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e2e4");
        let e5 = play(&mut g, e4, "e7e5");
        g.set_comment_before(e4, "before").expect("c");
        g.set_comment(e4, "after").expect("c");
        g.add_nag(e4, nag(1)).expect("n");
        g.set_comment_before(e5, "kept").expect("c");
        let t = g.truncate_start(e4).expect("start");
        assert_eq!(t.initial_comment(), Some("after"));
        assert_eq!(t.nags(t.root()).expect("n"), []);
        assert_eq!(t.comment_before(t.root()), Ok(None));
        let first = t.children(t.root()).expect("c")[0];
        assert_eq!(t.comment_before(first), Ok(Some("kept")));
    }

    #[test]
    fn truncate_after_removes_all_children_and_truncate_from_keeps_siblings() {
        let (mut g, [e4, e5, c5, _, _, _, e6]) = sample();
        let mut h = g.clone();
        // truncate_after(e4): main, variations and everything below go.
        g.truncate_after(e4).expect("after");
        assert_eq!(g.children(e4), Ok(&[][..]));
        assert_eq!(g.node_count(), 2);
        for id in [e5, c5, e6] {
            assert!(!g.contains(id));
        }
        check_invariants(&g);
        // truncate_from(e5): only e5 goes; the first sibling is promoted.
        h.truncate_from(e5).expect("from");
        assert_eq!(h.children(e4), Ok(&[c5, e6][..]));
        check_invariants(&h);
        // A leaf, and the root.
        assert_eq!(g.truncate_after(e4), Ok(()));
        g.truncate_after(g.root()).expect("root");
        assert_eq!(g.node_count(), 1);
        assert!(g.truncate_after(e4).is_err());
        check_invariants(&g);
    }

    #[test]
    fn add_move_lenient_keeps_castling_after_right_was_lost() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w Qkq - 0 1").expect("fen");
        let mut g = Game::from_position(pos.clone());
        let castle = pos
            .parse_san("O-O", crate::SanMode::Tolerant)
            .expect("san")
            .mv;
        assert!(g.add_move_new(g.root(), castle).is_err());
        let (id, created) = g.add_move_lenient(g.root(), castle).expect("lenient");
        assert!(created);
        assert_eq!(g.node_count(), 2);
        // Merge contract: same move again finds the child.
        assert_eq!(
            g.add_move_lenient(g.root(), castle).expect("again"),
            (id, false)
        );
    }

    #[test]
    fn add_move_lenient_still_rejects_illegal_moves() {
        let mut g = Game::new();
        let mut pos = Position::new();
        let e4 = pos.parse_uci("e2e4").expect("uci");
        pos.play(e4).expect("play");
        // `e2e4` again is illegal for the starting position's child state.
        let (id, _) = g.add_move_lenient(g.root(), e4).expect("e4");
        assert!(g.add_move_lenient(id, e4).is_err());
        assert_eq!(g.node_count(), 2);
    }

    #[test]
    fn find_child_and_add_move_new_report_creation() {
        let mut g = Game::new();
        let pos = Position::new();
        let e4 = pos.parse_uci("e2e4").expect("uci");
        let d4 = pos.parse_uci("d2d4").expect("uci");
        assert_eq!(g.find_child(g.root(), e4), Ok(None));
        let (a, created) = g.add_move_new(g.root(), e4).expect("add");
        assert!(created);
        assert_eq!(g.find_child(g.root(), e4), Ok(Some(a)));
        g.set_comment(a, "mine").expect("c");
        let (b, created) = g.add_move_new(g.root(), e4).expect("again");
        assert_eq!((b, created), (a, false));
        assert_eq!(g.comment(a), Ok(Some("mine")), "merge changes nothing");
        let (v, created) = g.add_move_new(g.root(), d4).expect("variation");
        assert!(created);
        assert_eq!(g.children(g.root()), Ok(&[a, v][..]));
        assert_eq!(
            g.find_child(fake(&g, 50), e4),
            Err(GameError::InvalidNode(fake(&g, 50)))
        );
        // An illegal move is an error, not a lookup miss.
        assert!(matches!(
            g.add_move_new(a, e4),
            Err(GameError::IllegalMove(_))
        ));
    }

    #[test]
    fn add_move_repeated_inside_a_variation_merges() {
        let (mut g, [_, _, c5, nf3v, c3, d6, _]) = sample();
        let before = g.node_count();
        assert_eq!(play(&mut g, c5, "g1f3"), nf3v);
        assert_eq!(play(&mut g, c5, "c2c3"), c3);
        assert_eq!(play(&mut g, nf3v, "d7d6"), d6);
        assert_eq!(g.node_count(), before);
        assert_eq!(g.children(c5), Ok(&[nf3v, c3][..]));
        check_invariants(&g);
    }

    #[test]
    fn same_content_compares_structure_and_ignores_ids() {
        let (g, [e4, _, _, _, _, d6, _]) = sample();
        let h = g.truncate_start(g.root()).expect("copy");
        assert!(g.same_content(&h));
        assert!(g.same_content(&g.clone()));

        // Different arena layout (slot reuse), same content.
        let mut edited = g.clone();
        edited.truncate_from(d6).expect("t");
        let nf3v_r = edited
            .children(edited.children(e4).expect("c")[1])
            .expect("c")[0];
        play(&mut edited, nf3v_r, "d7d6");
        assert!(g.same_content(&edited));
        assert_ne!(edited.nodes.len(), 0);
        edited.truncate_from(nf3v_r).expect("t");
        assert!(!g.same_content(&edited));

        let mut variants: Vec<Game> = Vec::new();
        let hroot = h.root();
        let h_e4 = h.children(hroot).expect("c")[0];
        let h_e5 = h.children(h_e4).expect("c")[0];
        let mut v = h.clone();
        v.set_comment(h_e5, "x").expect("c");
        variants.push(v);
        let mut v = h.clone();
        v.set_comment_before(h_e5, "x").expect("c");
        variants.push(v);
        let mut v = h.clone();
        v.add_nag(h_e5, nag(1)).expect("n");
        variants.push(v);
        let mut v = h.clone();
        v.set_initial_comment("x");
        variants.push(v);
        let mut v = h.clone();
        v.tags_mut().white = "Z".into();
        variants.push(v);
        let mut v = h.clone();
        v.promote_variation(h.children(h_e4).expect("c")[1])
            .expect("p");
        variants.push(v);
        let mut v = h.clone();
        v.truncate_after(h_e4).expect("t");
        variants.push(v);
        for (i, v) in variants.iter().enumerate() {
            assert!(!h.same_content(v), "variant {i} should differ");
        }
        // Same moves, different clocks at the start.
        let left =
            Game::from_position(Position::from_fen("8/8/8/8/8/5k2/8/R3K3 b - - 0 40").expect("f"));
        let right =
            Game::from_position(Position::from_fen("8/8/8/8/8/5k2/8/R3K3 b - - 5 41").expect("f"));
        assert_eq!(left.start_position(), right.start_position());
        assert!(!left.same_content(&right));
    }

    #[test]
    fn slot_is_small_and_node_size_is_reported() {
        let (slot, node) = (size_of::<Slot>(), size_of::<Node>());
        // A free slot must not carry a Node inline.
        assert!(slot <= 16, "Slot is {slot} bytes (Node is {node})");

        assert_eq!(size_of::<NodeId>(), 12);
    }
}
