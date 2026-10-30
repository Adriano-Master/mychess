//! Removing comments, variations and moves with a confirmation that counts
//! what goes (spec M2-4, D190 and rule 48).
//!
//! `GameMessage::RequestStrip` is what a menu or the context menu sends:
//! it counts, then asks (`Prompt::ConfirmStrip`) or acts at once when
//! `confirm` is off. The direct
//! messages (`StripComments`, `TruncateEnd`...) of the editing slice stay
//! as they are, for callers that already asked.
//!
//! Whether to ask is a parameter of the message (the front-end reads the
//! `confirm_destructive` preference), not part of `EditSettings`.

use mychess_core::{Game, NodeId};

use super::{GameSession, Notice, Prompt};

/// What a removal targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StripKind {
    /// Every comment of the game (moves and NAGs stay).
    Comments,
    /// Every variation of the game.
    Variations,
    /// Everything after the position of this node; the node's move stays.
    TruncateAfter(NodeId),
    /// This move and everything after it; its siblings stay.
    TruncateFrom(NodeId),
    /// The moves before the position of this node: it becomes the start of
    /// a new game (the tags stay).
    TruncateBefore(NodeId),
}

/// Number of comments of the game: after the moves, before them and the
/// initial one.
pub(crate) fn count_comments(game: &Game) -> usize {
    let root = game.root();
    let mut count = usize::from(game.initial_comment().is_some());
    if let Ok(nodes) = game.descendants(root) {
        for node in nodes {
            count += usize::from(game.comment(node).ok().flatten().is_some());
            count += usize::from(game.comment_before(node).ok().flatten().is_some());
        }
    }
    count
}

/// Number of variations of the game, at every level.
pub(crate) fn count_variations(game: &Game) -> usize {
    let Ok(nodes) = game.descendants(game.root()) else {
        return 0;
    };
    nodes
        .map(|node| game.children(node).map_or(0, |c| c.len().saturating_sub(1)))
        .sum::<usize>()
        + game
            .children(game.root())
            .map_or(0, |c| c.len().saturating_sub(1))
}

impl GameSession {
    /// How many items `kind` would remove; `None` for a stale node.
    pub fn strip_count(&self, kind: StripKind) -> Option<usize> {
        let game = &self.game;
        match kind {
            StripKind::Comments => Some(count_comments(game)),
            StripKind::Variations => Some(count_variations(game)),
            StripKind::TruncateAfter(node) => Some(game.descendants(node).ok()?.count()),
            StripKind::TruncateFrom(node) => {
                if node == game.root() {
                    return Some(0);
                }
                Some(game.descendants(node).ok()?.count() + 1)
            }
            StripKind::TruncateBefore(node) => game
                .ply(node)
                .ok()
                .map(|ply| usize::try_from(ply).unwrap_or(usize::MAX)),
        }
    }

    pub(super) fn request_strip(&mut self, kind: StripKind, confirm: bool) {
        let Some(count) = self.strip_count(kind) else {
            self.notice = Some(Notice::StaleNode);
            return;
        };
        if let StripKind::TruncateFrom(node) = kind
            && node == self.game.root()
        {
            self.notice = Some(Notice::NoMoveHere);
            return;
        }
        if count == 0 {
            self.notice = Some(Notice::NothingToStrip);
            return;
        }
        if confirm {
            self.open_prompt(Prompt::ConfirmStrip { kind, count });
        } else {
            self.apply_strip(kind);
        }
    }

    pub(super) fn answer_strip(&mut self, yes: bool) {
        let kind = match &self.prompt {
            Some(Prompt::ConfirmStrip { kind, .. }) => *kind,
            _ => return,
        };
        self.prompt = None;
        if yes {
            self.apply_strip(kind);
        }
    }

    fn apply_strip(&mut self, kind: StripKind) {
        match kind {
            StripKind::Comments => self.strip_comments(),
            StripKind::Variations => self.strip_variations(),
            StripKind::TruncateAfter(node) => {
                drop(self.edit(|game, _| game.truncate_after(node)));
            }
            StripKind::TruncateFrom(node) => {
                drop(self.edit(|game, _| game.truncate_from(node)));
            }
            StripKind::TruncateBefore(node) => {
                drop(self.edit(|game, cursor| {
                    let truncated = game.truncate_start(node)?;
                    *game = truncated;
                    *cursor = game.root();
                    Ok(())
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::{Game, Position};

    use super::*;
    use crate::session::GameMessage;

    /// `1.e4 {c} e5 (1...c5 {v}) 2.Nf3`, initial comment, a nested pair.
    fn game() -> (Game, [NodeId; 4]) {
        let mut g = Game::new();
        let e4 = g
            .add_move(g.root(), Position::new().parse_uci("e2e4").unwrap())
            .unwrap();
        let e5 = g
            .add_move(e4, g.position_at(e4).unwrap().parse_uci("e7e5").unwrap())
            .unwrap();
        let c5 = g
            .add_move(e4, g.position_at(e4).unwrap().parse_uci("c7c5").unwrap())
            .unwrap();
        let nf3 = g
            .add_move(e5, g.position_at(e5).unwrap().parse_uci("g1f3").unwrap())
            .unwrap();
        g.set_comment(e4, "c").unwrap();
        g.set_comment(c5, "v").unwrap();
        g.set_comment_before(c5, "b").unwrap();
        g.set_initial_comment("i");
        (g, [e4, e5, c5, nf3])
    }

    #[test]
    fn counts() {
        let (g, [e4, e5, c5, _]) = game();
        let s = GameSession::new(g);
        assert_eq!(s.strip_count(StripKind::Comments), Some(4));
        assert_eq!(s.strip_count(StripKind::Variations), Some(1));
        assert_eq!(s.strip_count(StripKind::TruncateAfter(e4)), Some(3));
        assert_eq!(s.strip_count(StripKind::TruncateFrom(e5)), Some(2));
        assert_eq!(s.strip_count(StripKind::TruncateBefore(c5)), Some(2));
        assert_eq!(
            s.strip_count(StripKind::TruncateBefore(s.game().root())),
            Some(0)
        );
    }

    #[test]
    fn request_asks_first_and_the_answer_decides() {
        let (g, _) = game();
        let mut s = GameSession::new(g);
        s.update(GameMessage::RequestStrip {
            kind: StripKind::Comments,
            confirm: true,
        });
        assert_eq!(
            s.prompt(),
            Some(&Prompt::ConfirmStrip {
                kind: StripKind::Comments,
                count: 4
            })
        );
        assert!(!s.is_dirty());
        s.update(GameMessage::AnswerStrip(false));
        assert!(s.prompt().is_none() && !s.is_dirty());
        s.update(GameMessage::RequestStrip {
            kind: StripKind::Comments,
            confirm: true,
        });
        s.update(GameMessage::AnswerStrip(true));
        assert_eq!(count_comments(s.game()), 0);
        assert!(s.is_dirty());
    }

    #[test]
    fn nothing_to_strip_opens_no_prompt() {
        let mut s = GameSession::default();
        s.update(GameMessage::RequestStrip {
            kind: StripKind::Variations,
            confirm: true,
        });
        assert!(s.prompt().is_none());
        assert_eq!(s.notice(), Some(&Notice::NothingToStrip));
    }

    #[test]
    fn without_confirmation_it_acts_at_once() {
        let (g, [e4, ..]) = game();
        let mut s = GameSession::new(g);
        s.update(GameMessage::RequestStrip {
            kind: StripKind::TruncateAfter(e4),
            confirm: false,
        });
        assert!(s.prompt().is_none());
        assert_eq!(s.game().node_count(), 2);
    }

    #[test]
    fn truncate_before_starts_a_new_game_at_the_node() {
        let (g, [_, e5, ..]) = game();
        let mut s = GameSession::new(g);
        s.update(GameMessage::RequestStrip {
            kind: StripKind::TruncateBefore(e5),
            confirm: true,
        });
        s.update(GameMessage::AnswerStrip(true));
        assert_eq!(s.cursor(), s.game().root());
        assert_eq!(s.game().node_count(), 2);
        assert_eq!(s.game().start_position().fullmove_number(), 2);
    }

    #[test]
    fn truncate_from_the_root_is_refused() {
        let (g, _) = game();
        let mut s = GameSession::new(g);
        let root = s.game().root();
        s.update(GameMessage::RequestStrip {
            kind: StripKind::TruncateFrom(root),
            confirm: true,
        });
        assert!(s.prompt().is_none());
        assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
    }

    #[test]
    fn dismissing_the_prompt_cancels() {
        let (g, _) = game();
        let mut s = GameSession::new(g);
        s.update(GameMessage::RequestStrip {
            kind: StripKind::Variations,
            confirm: true,
        });
        s.update(GameMessage::DismissPrompt);
        assert!(s.prompt().is_none());
        assert_eq!(count_variations(s.game()), 1);
    }

    #[test]
    fn stale_ids_are_a_notice() {
        let (g, [_, _, c5, _]) = game();
        let mut s = GameSession::new(g);
        s.update(GameMessage::DeleteVariation(
            crate::session::VariationTarget::Node(c5),
        ));
        s.update(GameMessage::RequestStrip {
            kind: StripKind::TruncateAfter(c5),
            confirm: true,
        });
        assert_eq!(s.notice(), Some(&Notice::StaleNode));
    }
}
