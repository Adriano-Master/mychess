//! Navigation and the variation chooser (spec rules 9-25).

use mychess_core::{Color, NodeId};

use super::message::Nav;
use super::{GameSession, Notice};

/// Longest preview line of the variation chooser, in moves.
const PREVIEW_MOVES: usize = 5;

/// The open variation chooser (Scid: the variation window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariationChooser {
    /// The node whose continuations are listed.
    pub parent: NodeId,
    /// The highlighted row; 0 is the main continuation.
    pub selected: usize,
}

/// One row of the variation chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariationChoice {
    /// Row number; 0 is the main continuation.
    pub index: usize,
    /// Up to five moves of the line, in SAN (move numbers are up to the GUI).
    pub san: Vec<String>,
    /// The first node of the line.
    pub node: NodeId,
}

impl GameSession {
    /// The closest ancestor-or-self of `node` that heads a variation.
    pub(super) fn line_head(&self, node: NodeId) -> Option<NodeId> {
        let mut current = node;
        loop {
            if self.game.is_variation(current).unwrap_or(false) {
                return Some(current);
            }
            match self.game.parent(current) {
                Ok(Some(parent)) => current = parent,
                _ => return None,
            }
        }
    }

    /// The position the line of `node` starts from (the root on the main
    /// line).
    fn line_start(&self, node: NodeId) -> NodeId {
        self.line_head(node)
            .and_then(|head| self.game.parent(head).ok().flatten())
            .unwrap_or_else(|| self.game.root())
    }

    /// Follows the first continuation down to a leaf.
    fn leaf_from(&self, node: NodeId) -> NodeId {
        let mut current = node;
        while let Some(&next) = self.children_of(current).first() {
            current = next;
        }
        current
    }

    pub(super) fn navigate(&mut self, nav: Nav) {
        let cursor = self.cursor;
        match nav {
            Nav::Start => self.move_to(self.game.root()),
            Nav::GameEnd => self.move_to(self.leaf_from(self.game.root())),
            Nav::Home => self.move_to(self.line_start(cursor)),
            Nav::End => {
                let target = if !self.children_of(cursor).is_empty() {
                    self.leaf_from(cursor)
                } else if self.line_head(cursor).is_some() {
                    self.leaf_from(self.game.root())
                } else {
                    cursor
                };
                self.move_to(target);
            }
            Nav::Back(n) => self.back(n),
            Nav::Forward(n) => self.forward(n),
            Nav::ExitVariation => {
                if let Some(head) = self.line_head(cursor)
                    && let Ok(Some(parent)) = self.game.parent(head)
                {
                    self.move_to(parent);
                }
            }
            Nav::EnterVariation(index) => {
                if let Some(&child) = self.children_of(cursor).get(index) {
                    self.move_to(child);
                }
            }
            Nav::GoToNode(node) => {
                if self.game.contains(node) {
                    self.move_to(node);
                } else {
                    self.notice = Some(Notice::StaleNode);
                }
            }
            Nav::GoToMainlinePly(ply) => {
                let target = match usize::try_from(ply).ok().and_then(|p| p.checked_sub(1)) {
                    None => self.game.root(),
                    Some(index) => self
                        .game
                        .mainline()
                        .nth(index)
                        .unwrap_or_else(|| self.leaf_from(self.game.root())),
                };
                self.move_to(target);
            }
            Nav::GoToMoveNumber(number) => self.go_to_move_number(number, Color::White),
            Nav::GoToBlackMoveNumber(number) => self.go_to_move_number(number, Color::Black),
        }
    }

    /// Back `n` moves; stops after leaving the head of a variation.
    fn back(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        let mut current = self.cursor;
        for _ in 0..n {
            let Ok(Some(parent)) = self.game.parent(current) else {
                break;
            };
            let was_head = self.game.is_variation(current).unwrap_or(false);
            current = parent;
            if was_head {
                break;
            }
        }
        self.move_to(current);
    }

    fn forward(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        if n == 1
            && self.settings.show_var_popup
            && !self.autoplay.running
            && self.children_of(self.cursor).len() >= 2
        {
            self.reset_interaction();
            self.chooser = Some(VariationChooser {
                parent: self.cursor,
                selected: 0,
            });
            return;
        }
        let mut current = self.cursor;
        for _ in 0..n {
            match self.children_of(current).first() {
                Some(&next) => current = next,
                None => break,
            }
        }
        self.move_to(current);
    }

    /// `Ctrl+G`: the position after the move `number` of `color` in the
    /// main line. Before the first such move: the root; after the last: the
    /// leaf.
    fn go_to_move_number(&mut self, number: u32, color: Color) {
        if number == 0 {
            return;
        }
        let mut previous = self.game.root();
        let mut target = None;
        for node in self.game.mainline() {
            let Ok(position) = self.game.position_at(previous) else {
                break;
            };
            if position.turn() == color && position.fullmove_number() == number {
                target = Some(node);
                break;
            }
            previous = node;
        }
        let target = target.unwrap_or_else(|| {
            let root = self.game.start_position();
            let key = |fullmove: u32, turn: Color| (fullmove, u8::from(turn == Color::Black));
            if key(number, color) < key(root.fullmove_number(), root.turn()) {
                self.game.root()
            } else {
                self.leaf_from(self.game.root())
            }
        });
        self.move_to(target);
    }

    // ---- variation chooser ----

    pub(super) fn open_chooser(&mut self) {
        if self.children_of(self.cursor).len() >= 2 {
            self.reset_interaction();
            self.chooser = Some(VariationChooser {
                parent: self.cursor,
                selected: 0,
            });
        }
    }

    pub(super) fn chooser_move(&mut self, delta: isize) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        let count = self.children_of(chooser.parent).len();
        let last = isize::try_from(count.saturating_sub(1)).unwrap_or(isize::MAX);
        let current = isize::try_from(chooser.selected).unwrap_or(isize::MAX);
        let next = current.saturating_add(delta).clamp(0, last);
        let next = usize::try_from(next).unwrap_or(0);
        if let Some(chooser) = &mut self.chooser {
            chooser.selected = next;
        }
    }

    pub(super) fn chooser_confirm(&mut self, index: Option<usize>) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        let index = index.unwrap_or(chooser.selected);
        let target = self.children_of(chooser.parent).get(index).copied();
        if let Some(node) = target {
            self.move_to(node);
        }
    }

    /// One row per continuation of the chooser's node (or of the cursor when
    /// the chooser is closed), with up to five SAN moves each.
    pub fn variation_choices(&self) -> Vec<VariationChoice> {
        let parent = self.chooser.as_ref().map_or(self.cursor, |c| c.parent);
        self.children_of(parent)
            .iter()
            .enumerate()
            .map(|(index, &node)| VariationChoice {
                index,
                san: self.preview(node),
                node,
            })
            .collect()
    }

    fn preview(&self, first: NodeId) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = Some(first);
        while let Some(node) = current
            && out.len() < PREVIEW_MOVES
        {
            let san = self
                .game
                .parent(node)
                .ok()
                .flatten()
                .and_then(|parent| self.game.position_at(parent).ok())
                .zip(self.game.mv(node).ok())
                .and_then(|(position, mv)| position.san(mv).ok());
            match san {
                Some(san) => out.push(san),
                None => break,
            }
            current = self.children_of(node).first().copied();
        }
        out
    }
}
