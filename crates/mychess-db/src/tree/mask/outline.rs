// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::mask::displayMask`
// and `populateDisplayMask`, 2552-2777). Copyright (c) 1999-2002 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The outline of a mask (decision D1833): the lines the mask holds from a
//! position, as a tree of moves where runs of single moves are merged.

use std::collections::HashSet;

use mychess_core::Position;

use super::matching::resolve_move;
use super::model::{
    MAX_PREVIEW_BYTES, Marker, MaskColor, MaskKey, MaskMove, MaskNag, TreeMask, first_line,
};

/// The deepest outline [`build`] makes, whatever the budget asks (D1851): the
/// outline is built by recursion, and a line this long is not readable.
pub const MAX_OUTLINE_DEPTH: usize = 1000;

/// The limits of [`build`] (D1833).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineBudget {
    /// The most moves (steps) in the outline.
    pub max_nodes: usize,
    /// The deepest line, in moves. Values above [`MAX_OUTLINE_DEPTH`] are
    /// taken as that.
    pub max_depth: usize,
}

impl Default for OutlineBudget {
    fn default() -> OutlineBudget {
        OutlineBudget {
            max_nodes: 20_000,
            max_depth: 200,
        }
    }
}

/// One move of an outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineStep {
    /// The first line of the comment of the position the move is played
    /// from, at most [`MAX_PREVIEW_BYTES`] bytes (empty if none).
    pub position_comment: String,
    /// The move, as stored.
    pub san: String,
    /// Its NAG.
    pub nag: Option<MaskNag>,
    /// Its color.
    pub color: MaskColor,
    /// Its markers.
    pub markers: [Option<Marker>; 2],
    /// The first line of its comment, at most [`MAX_PREVIEW_BYTES`] bytes
    /// (D1851); the full text is in the mask.
    pub comment: String,
    /// The position reached by the move (what a double click goes to);
    /// `None` when the entry is illegal in its position.
    pub after: Option<MaskKey>,
}

/// A node of the outline: a run of moves that follow each other with no
/// alternative (the Scid shows it on one line), then the alternatives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    /// The merged moves, at least one.
    pub steps: Vec<OutlineStep>,
    /// The alternatives after the last step (two or more, or none).
    pub children: Vec<OutlineNode>,
    /// The line goes on but was cut by the budget (the interface shows
    /// `...`).
    pub truncated: bool,
}

/// The result of [`build`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    /// The first line of the comment of the starting position (at most
    /// [`MAX_PREVIEW_BYTES`] bytes).
    pub comment: String,
    /// The moves of the mask at the starting position.
    pub roots: Vec<OutlineNode>,
    /// The number of steps in the outline.
    pub nodes: usize,
    /// Whether the budget cut something.
    pub truncated: bool,
}

struct Builder<'a> {
    mask: &'a TreeMask,
    budget: OutlineBudget,
    nodes: usize,
    truncated: bool,
    /// Positions on the path from the root to the current node (cycles).
    path: HashSet<MaskKey>,
}

/// Builds the outline of the lines of `mask` from `position`. A position
/// already on the path from the start is not expanded again (D1833), and the
/// siblings of that move are kept. Comments are kept as previews (first
/// line, [`MAX_PREVIEW_BYTES`]), and the depth is at most
/// [`MAX_OUTLINE_DEPTH`].
pub fn build(mask: &TreeMask, position: &Position, budget: OutlineBudget) -> Outline {
    let key = MaskKey::from_position(position);
    let budget = OutlineBudget {
        max_depth: budget.max_depth.min(MAX_OUTLINE_DEPTH),
        ..budget
    };
    let mut b = Builder {
        mask,
        budget,
        nodes: 0,
        truncated: false,
        path: HashSet::new(),
    };
    let comment = mask
        .get(&key)
        .map(|p| first_line(&p.comment, MAX_PREVIEW_BYTES))
        .unwrap_or_default();
    b.path.insert(key.clone());
    let (roots, _) = b.children_of(&key, position, 0);
    Outline {
        comment,
        roots,
        nodes: b.nodes,
        truncated: b.truncated,
    }
}

impl<'a> Builder<'a> {
    fn visible(&self, key: &MaskKey) -> Vec<&'a MaskMove> {
        let mask: &'a TreeMask = self.mask;
        mask.get(key)
            .map(|p| p.visible_moves().collect())
            .unwrap_or_default()
    }

    fn comment_of(&self, key: &MaskKey) -> &'a str {
        let mask: &'a TreeMask = self.mask;
        mask.get(key).map_or("", |p| p.comment.as_str())
    }

    fn over_budget(&self, depth: usize) -> bool {
        self.nodes >= self.budget.max_nodes || depth >= self.budget.max_depth
    }

    /// The nodes for the visible moves of the position `pos` (key `key`,
    /// already on the path), and whether the budget cut some.
    fn children_of(
        &mut self,
        key: &MaskKey,
        pos: &Position,
        depth: usize,
    ) -> (Vec<OutlineNode>, bool) {
        let comment = first_line(self.comment_of(key), MAX_PREVIEW_BYTES);
        let mut out = Vec::new();
        for m in self.visible(key) {
            if self.over_budget(depth) {
                self.truncated = true;
                return (out, true);
            }
            out.push(self.node(&comment, m, pos, depth));
        }
        (out, false)
    }

    /// One step, and the position after it when the move is legal.
    fn step(
        &mut self,
        position_comment: &str,
        m: &MaskMove,
        pos: &Position,
    ) -> (OutlineStep, Option<Position>) {
        let next = resolve_move(pos, &m.san).and_then(|mv| pos.after(mv).ok());
        self.nodes += 1;
        let step = OutlineStep {
            position_comment: position_comment.to_owned(),
            san: m.san.clone(),
            nag: m.nag.clone(),
            color: m.color.clone(),
            markers: m.markers.clone(),
            comment: first_line(&m.comment, MAX_PREVIEW_BYTES),
            after: next.as_ref().map(MaskKey::from_position),
        };
        (step, next)
    }

    fn node(
        &mut self,
        position_comment: &str,
        m: &MaskMove,
        pos: &Position,
        depth: usize,
    ) -> OutlineNode {
        let (first, next) = self.step(position_comment, m, pos);
        let mut node = OutlineNode {
            steps: vec![first],
            children: Vec::new(),
            truncated: false,
        };
        let Some(mut cur) = next else {
            return node;
        };
        let mut cur_key = MaskKey::from_position(&cur);
        if self.path.contains(&cur_key) {
            return node;
        }
        self.path.insert(cur_key.clone());
        let mut added = vec![cur_key.clone()];
        let mut depth = depth + 1;
        let mut expandable = true;
        loop {
            let moves = self.visible(&cur_key);
            let [only] = moves.as_slice() else {
                break;
            };
            if self.over_budget(depth) {
                self.truncated = true;
                node.truncated = true;
                expandable = false;
                break;
            }
            let comment = first_line(self.comment_of(&cur_key), MAX_PREVIEW_BYTES);
            let (step, next) = self.step(&comment, only, &cur);
            node.steps.push(step);
            depth += 1;
            let Some(n) = next else {
                expandable = false;
                break;
            };
            let key = MaskKey::from_position(&n);
            if self.path.contains(&key) {
                expandable = false;
                break;
            }
            self.path.insert(key.clone());
            added.push(key.clone());
            cur = n;
            cur_key = key;
        }
        if expandable {
            let (children, cut) = self.children_of(&cur_key, &cur, depth);
            node.children = children;
            node.truncated = cut;
        }
        for k in &added {
            self.path.remove(k);
        }
        node
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::mask::{MaskOp, mask_san};
    use mychess_core::SanMode;

    fn add_line(mask: &mut TreeMask, moves: &[&str]) {
        let mut pos = Position::new();
        for san in moves {
            let mv = pos.parse_san(san, SanMode::Strict).unwrap().mv;
            let text = mask_san(&pos, mv).unwrap();
            mask.apply(&pos, &MaskOp::AddMove { san: text });
            pos.play(mv).unwrap();
        }
    }

    fn texts(n: &OutlineNode) -> Vec<&str> {
        n.steps.iter().map(|s| s.san.as_str()).collect()
    }

    #[test]
    fn single_move_runs_are_merged_and_forks_are_children() {
        let mut m = TreeMask::new();
        add_line(&mut m, &["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        add_line(&mut m, &["e4", "e5", "Nf3", "Nc6", "Bc4"]);
        add_line(&mut m, &["d4"]);
        let o = build(&m, &Position::new(), OutlineBudget::default());
        assert_eq!(o.roots.len(), 2);
        assert_eq!(texts(&o.roots[0]), ["e4", "e5", "Nf3", "Nc6"]);
        assert_eq!(o.roots[0].children.len(), 2);
        assert_eq!(texts(&o.roots[0].children[1]), ["Bc4"]);
        assert_eq!(texts(&o.roots[1]), ["d4"]);
        assert_eq!(o.nodes, 7);
        assert!(!o.truncated);
        assert!(o.roots[0].steps[3].after.is_some());
    }

    #[test]
    fn position_comments_and_marks_are_carried() {
        let mut m = TreeMask::new();
        add_line(&mut m, &["e4", "e5"]);
        let start = Position::new();
        m.apply(
            &start,
            &MaskOp::SetPositionComment {
                text: "root".into(),
            },
        );
        let mut after = start.clone();
        after.play(start.parse_uci("e2e4").unwrap()).unwrap();
        m.apply(&after, &MaskOp::SetPositionComment { text: "mid".into() });
        m.apply(
            &after,
            &MaskOp::SetNag {
                san: "e5".into(),
                nag: Some(MaskNag::Good),
            },
        );
        let o = build(&m, &start, OutlineBudget::default());
        assert_eq!(o.comment, "root");
        let steps = &o.roots[0].steps;
        assert_eq!(steps[0].position_comment, "root");
        assert_eq!(steps[1].position_comment, "mid");
        assert_eq!(steps[1].nag, Some(MaskNag::Good));
    }

    #[test]
    fn a_cycle_is_not_expanded_and_siblings_are_kept() {
        // Nf3 Nf6 Ng1 Ng8 returns to the start position.
        let mut m = TreeMask::new();
        add_line(&mut m, &["Nf3", "Nf6", "Ng1", "Ng8"]);
        add_line(&mut m, &["e4"]);
        let o = build(&m, &Position::new(), OutlineBudget::default());
        assert_eq!(o.roots.len(), 2, "the sibling e4 is still there");
        assert_eq!(texts(&o.roots[0]), ["Nf3", "Nf6", "Ng1", "Ng8"]);
        assert_eq!(o.roots[0].children.len(), 0);
        assert_eq!(texts(&o.roots[1]), ["e4"]);
    }

    #[test]
    fn transposition_cycle_inside_a_fork() {
        let mut m = TreeMask::new();
        add_line(&mut m, &["e4", "e5", "Nf3", "Nf6", "Ng1", "Ng8"]);
        add_line(&mut m, &["e4", "e5", "Nc3"]);
        let o = build(&m, &Position::new(), OutlineBudget::default());
        let root = &o.roots[0];
        assert_eq!(texts(root), ["e4", "e5"]);
        let kids: Vec<_> = root.children.iter().map(texts).collect();
        assert_eq!(kids[0], ["Nf3", "Nf6", "Ng1", "Ng8"]);
        assert_eq!(kids[1], ["Nc3"], "sibling after the cycle is kept");
    }

    #[test]
    fn budget_is_respected_and_reported() {
        let mut m = TreeMask::new();
        add_line(&mut m, &["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
        let o = build(
            &m,
            &Position::new(),
            OutlineBudget {
                max_nodes: 3,
                max_depth: 200,
            },
        );
        assert!(o.truncated);
        assert!(o.nodes <= 3);
        assert!(o.roots[0].truncated);
        let o = build(
            &m,
            &Position::new(),
            OutlineBudget {
                max_nodes: 100,
                max_depth: 2,
            },
        );
        assert!(o.truncated);
        assert_eq!(texts(&o.roots[0]).len(), 2);
    }

    #[test]
    fn illegal_entries_end_the_line_and_null_is_hidden() {
        let mut m = TreeMask::new();
        let start = Position::new();
        m.apply(&start, &MaskOp::AddMove { san: "null".into() });
        m.apply(&start, &MaskOp::AddMove { san: "Qd5".into() });
        let o = build(&m, &start, OutlineBudget::default());
        assert_eq!(o.roots.len(), 1);
        assert_eq!(o.roots[0].steps[0].after, None);
        let nothing = build(&TreeMask::new(), &start, OutlineBudget::default());
        assert_eq!(nothing.roots.len(), 0);
    }
}
