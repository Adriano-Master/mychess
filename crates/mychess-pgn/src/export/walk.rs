//! Depth-first walk of a game tree for the document writers.
//!
//! The walk uses an explicit stack, so a game with variations nested to any
//! depth cannot overflow the call stack.

use mychess_core::{Game, GameError, Nag, NodeId, Position};

use crate::write::WriteError;

/// One move of the walk.
pub(crate) struct MoveEv<'g> {
    pub(crate) depth: usize,
    /// First move of its line (main line or variation).
    pub(crate) first_in_line: bool,
    /// SAN with check/mate suffix (`--` for a null move).
    pub(crate) san: String,
    pub(crate) is_null: bool,
    /// Full move number of the position before the move.
    pub(crate) number: u32,
    pub(crate) white: bool,
    pub(crate) nags: &'g [Nag],
    pub(crate) comment: Option<&'g str>,
    /// Comment that precedes the move (only used on the first move of a
    /// variation).
    pub(crate) comment_before: Option<&'g str>,
    pub(crate) after: &'g Position,
}

pub(crate) enum Ev<'g> {
    Move(MoveEv<'g>),
    /// A variation opens.
    VarStart,
    VarEnd,
}

struct Frame {
    cur: Option<NodeId>,
    /// Variations still to open after the move just emitted, last first.
    vars: Vec<NodeId>,
    depth: usize,
    first: bool,
}

/// Walks `game` calling `f` for every move and variation boundary.
/// Variations are skipped when `variations` is false.
pub(crate) fn walk<'g>(
    game: &'g Game,
    variations: bool,
    mut f: impl FnMut(Ev<'g>),
) -> Result<(), WriteError> {
    let root_child = game.children(game.root())?.first().copied();
    let mut stack = vec![Frame {
        cur: root_child,
        vars: Vec::new(),
        depth: 0,
        first: true,
    }];
    while let Some(top) = stack.last_mut() {
        if let Some(v) = top.vars.pop() {
            let depth = top.depth + 1;
            f(Ev::VarStart);
            stack.push(Frame {
                cur: Some(v),
                vars: Vec::new(),
                depth,
                first: true,
            });
            continue;
        }
        if let Some(m) = top.cur.take() {
            let parent = game.parent(m)?.ok_or(GameError::IsRoot)?;
            let before = game.position_at(parent)?;
            let after = game.position_at(m)?;
            let mv = game.mv(m)?;
            let san = before.san(mv).map_err(|source| WriteError::Move {
                ply: game.ply(m).unwrap_or(0),
                source,
            })?;
            let siblings = game.children(parent)?;
            let is_main = siblings.first() == Some(&m);
            let depth = top.depth;
            let first_in_line = top.first;
            top.first = false;
            top.cur = game.children(m)?.first().copied();
            if is_main && variations {
                top.vars = siblings.iter().skip(1).rev().copied().collect();
            }
            f(Ev::Move(MoveEv {
                depth,
                first_in_line,
                is_null: mv.is_null(),
                san,
                number: before.fullmove_number(),
                white: before.turn() == mychess_core::Color::White,
                nags: game.nags(m)?,
                comment: game.comment(m)?,
                comment_before: game.comment_before(m)?,
                after,
            }));
            continue;
        }
        let depth = top.depth;
        stack.pop();
        if depth > 0 {
            f(Ev::VarEnd);
        }
    }
    Ok(())
}
