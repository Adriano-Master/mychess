//! Adding what an engine found to the game (spec M4-4, D440-D443).
//!
//! The three messages go through the funnel ([`GameSession::edit`]), so each
//! action is **one** undo point and the cursor never moves (D443).

use mychess_core::{Move, NodeId};

use super::{GameSession, Notice};
use crate::analysis::AnalysisNotice;

/// One line to add: the moves and the comment that goes before the first new
/// move (D441).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLineToAdd {
    /// The moves, from the position of the node the action is made at.
    pub pv: Vec<Move>,
    /// The comment (`depth:score`), put before the first move that was not
    /// in the game already.
    pub comment: Option<String>,
}

impl GameSession {
    /// Adds `lines` as variations of the position of `node`.
    ///
    /// - A stale `node` gives [`Notice::StaleNode`]; no line, nothing.
    /// - A move that is already a continuation is followed, not duplicated
    ///   (the tree merges equal siblings); the comment goes to the first
    ///   node that is new, so a line that is entirely in the game adds
    ///   nothing.
    /// - At the **end of a variation** the line extends the variation and
    ///   only the first line is used; at the **end of the main line** the
    ///   action is refused ([`AnalysisNotice::AtGameEnd`], D442), except in
    ///   a game with no move, where the line becomes the main line.
    /// - A move that is not playable stops its line; the moves before it
    ///   stay and [`AnalysisNotice::LineTruncated`] says how many.
    pub(super) fn add_engine_lines(&mut self, node: NodeId, mut lines: Vec<EngineLineToAdd>) {
        if !self.game.contains(node) {
            self.notice = Some(Notice::StaleNode);
            return;
        }
        lines.retain(|line| !line.pv.is_empty());
        if lines.is_empty() {
            return;
        }
        if self.children_of(node).is_empty() {
            let on_main_line = self.game.mainline().any(|id| id == node);
            if on_main_line {
                self.analysis_notice = Some(AnalysisNotice::AtGameEnd);
                return;
            }
            lines.truncate(1);
        }
        let mut truncated: Option<usize> = None;
        drop(self.edit(|game, _| {
            for line in &lines {
                let mut current = node;
                let mut first_new = None;
                for (applied, &mv) in line.pv.iter().enumerate() {
                    let Ok((id, created)) = game.add_move_new(current, mv) else {
                        truncated.get_or_insert(applied);
                        break;
                    };
                    if created && first_new.is_none() {
                        first_new = Some(id);
                    }
                    current = id;
                }
                if let (Some(id), Some(text)) = (first_new, line.comment.as_deref()) {
                    game.set_comment_before(id, text)?;
                }
            }
            Ok(())
        }));
        if let Some(applied) = truncated {
            self.analysis_notice = Some(AnalysisNotice::LineTruncated(applied));
        }
    }

    /// Puts `text` in front of the comment after the move of `node` (the
    /// initial comment for the root), as `"$text $comment"` (`addAnalysisScore`).
    pub(super) fn prepend_comment(&mut self, node: NodeId, text: &str) {
        let Ok(current) = self.game.comment(node).map(|c| c.map(str::to_owned)) else {
            self.notice = Some(Notice::StaleNode);
            return;
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let joined = match current {
            Some(old) if !old.trim().is_empty() => format!("{text} {old}"),
            _ => text.to_owned(),
        };
        drop(self.edit(|game, _| game.set_comment(node, &joined)));
    }

    /// The remark of the last message about adding an engine line, if any.
    pub fn analysis_notice(&self) -> Option<&AnalysisNotice> {
        self.analysis_notice.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::Game;

    use super::*;
    use crate::session::GameMessage;

    /// The moves of `ucis` played from `from`, as a game path; returns the
    /// last node.
    fn play(game: &mut Game, from: NodeId, ucis: &[&str]) -> NodeId {
        let mut node = from;
        for uci in ucis {
            let mv = game
                .position_at(node)
                .expect("position")
                .parse_uci(uci)
                .expect("uci");
            node = game.add_move(node, mv).expect("legal");
        }
        node
    }

    fn pv(game: &Game, node: NodeId, ucis: &[&str]) -> Vec<Move> {
        let mut position = game.position_at(node).expect("position").clone();
        ucis.iter()
            .map(|uci| {
                let mv = position.parse_uci(uci).expect("uci");
                position.play(mv).ok();
                mv
            })
            .collect()
    }

    /// 1.e4 e5 2.Nf3 Nc6, cursor at the end.
    fn italian() -> (GameSession, Vec<NodeId>) {
        let mut game = Game::new();
        let root = game.root();
        let e4 = play(&mut game, root, &["e2e4"]);
        let e5 = play(&mut game, e4, &["e7e5"]);
        let nf3 = play(&mut game, e5, &["g1f3"]);
        let nc6 = play(&mut game, nf3, &["b8c6"]);
        let mut session = GameSession::new(game);
        session.update(GameMessage::Navigate(crate::Nav::GameEnd));
        (session, vec![root, e4, e5, nf3, nc6])
    }

    fn line(game: &Game, node: NodeId, ucis: &[&str], comment: &str) -> EngineLineToAdd {
        EngineLineToAdd {
            pv: pv(game, node, ucis),
            comment: Some(comment.to_owned()),
        }
    }

    fn add(session: &mut GameSession, node: NodeId, lines: Vec<EngineLineToAdd>) {
        session.update(GameMessage::AddEngineLines { node, lines });
    }

    #[test]
    fn a_line_in_the_middle_follows_the_game_and_branches_where_it_differs() {
        let (mut session, nodes) = italian();
        let (e4, nf3) = (nodes[1], nodes[3]);
        let lines = vec![line(
            session.game(),
            e4,
            &["e7e5", "g1f3", "f8c5"],
            "20:+0.35",
        )];
        let cursor = session.cursor();
        add(&mut session, e4, lines);
        let game = session.game();
        // `e5` and `Nf3` were followed, not duplicated.
        assert_eq!(game.node_count(), 6, "only Bc5 is new");
        let children = game.children(nf3).expect("children");
        assert_eq!(children.len(), 2);
        let bc5 = children[1];
        assert_eq!(game.comment_before(bc5).expect("node"), Some("20:+0.35"));
        assert_eq!(game.comment(bc5).expect("node"), None);
        assert_eq!(session.cursor(), cursor, "D443: the cursor stays");
        assert_eq!(session.analysis_notice(), None);
    }

    #[test]
    fn a_new_first_move_makes_a_variation_with_the_comment_before_it() {
        let (mut session, nodes) = italian();
        let root = nodes[0];
        let lines = vec![line(session.game(), root, &["c2c4", "e7e5"], "[SF] 18:M3")];
        add(&mut session, root, lines);
        let game = session.game();
        let children = game.children(root).expect("children");
        assert_eq!(children.len(), 2);
        let c4 = children[1];
        assert_eq!(game.comment_before(c4).expect("node"), Some("[SF] 18:M3"));
        assert_eq!(game.children(c4).expect("children").len(), 1);
    }

    #[test]
    fn one_undo_removes_a_whole_action() {
        let (mut session, nodes) = italian();
        let before = session.game().clone();
        let depth = session.undo_depth();
        let lines = vec![
            line(session.game(), nodes[0], &["d2d4", "d7d5"], "a"),
            line(session.game(), nodes[0], &["c2c4"], "b"),
        ];
        add(&mut session, nodes[0], lines);
        assert_eq!(session.undo_depth(), depth + 1);
        session.update(GameMessage::Undo);
        assert!(session.game().same_content(&before));
        assert!(session.game().contains(session.cursor()), "no stale id");
        session.update(GameMessage::Redo);
        assert_eq!(session.game().node_count(), before.node_count() + 3);
    }

    #[test]
    fn add_all_follows_a_line_that_is_already_there_without_comment() {
        let (mut session, nodes) = italian();
        let root = nodes[0];
        let lines = vec![
            line(session.game(), root, &["e2e4"], "a"),
            line(session.game(), root, &["d2d4"], "b"),
            line(session.game(), root, &["c2c4"], "c"),
        ];
        add(&mut session, root, lines);
        let game = session.game();
        let children = game.children(root).expect("children");
        assert_eq!(children.len(), 3);
        assert_eq!(game.comment_before(children[0]).expect("node"), None);
        assert_eq!(game.comment_before(children[1]).expect("node"), Some("b"));
        assert_eq!(game.comment_before(children[2]).expect("node"), Some("c"));
    }

    #[test]
    fn a_line_that_is_all_in_the_game_changes_nothing_and_leaves_no_undo_point() {
        let (mut session, nodes) = italian();
        let depth = session.undo_depth();
        let lines = vec![line(session.game(), nodes[0], &["e2e4", "e7e5"], "a")];
        add(&mut session, nodes[0], lines);
        assert_eq!(session.undo_depth(), depth);
    }

    #[test]
    fn the_end_of_the_main_line_refuses_the_line() {
        let (mut session, nodes) = italian();
        let leaf = nodes[4];
        let before = session.game().clone();
        let depth = session.undo_depth();
        let lines = vec![line(session.game(), leaf, &["f1c4", "f8c5"], "a")];
        add(&mut session, leaf, lines);
        assert_eq!(session.analysis_notice(), Some(&AnalysisNotice::AtGameEnd));
        assert!(session.game().same_content(&before));
        assert_eq!(session.undo_depth(), depth, "nothing was recorded to undo");
    }

    #[test]
    fn an_empty_game_takes_the_line_as_its_main_line() {
        let mut session = GameSession::default();
        let root = session.game().root();
        let lines = vec![line(session.game(), root, &["e2e4", "e7e5"], "a")];
        add(&mut session, root, lines);
        assert_eq!(session.analysis_notice(), None);
        assert_eq!(session.game().mainline().count(), 2);
    }

    #[test]
    fn the_end_of_a_variation_is_extended_with_the_first_line_only() {
        // 1.e4 e5 2.Nf3 (2.f4), the cursor on 2.f4.
        let (italian, nodes) = italian();
        let mut game = italian.game().clone();
        let f4 = play(&mut game, nodes[2], &["f2f4"]);
        let mut session = GameSession::new(game);
        let lines = vec![
            line(session.game(), f4, &["e5f4", "g1f3"], "first"),
            line(session.game(), f4, &["d7d5"], "second"),
        ];
        add(&mut session, f4, lines);
        let game = session.game();
        let next = game.children(f4).expect("children");
        assert_eq!(next.len(), 1, "extended, not branched: {next:?}");
        assert_eq!(game.comment_before(next[0]).expect("node"), Some("first"));
        assert_eq!(game.children(next[0]).expect("children").len(), 1);
        assert_eq!(session.analysis_notice(), None);
    }

    #[test]
    fn an_unplayable_move_keeps_what_came_before_and_says_how_many() {
        let (mut session, nodes) = italian();
        let root = nodes[0];
        let mut moves = pv(session.game(), root, &["d2d4", "d7d5"]);
        // The same move again: illegal in the position after 1.d4 d5.
        moves.push(moves[0]);
        let lines = vec![EngineLineToAdd {
            pv: moves,
            comment: Some("t".to_owned()),
        }];
        add(&mut session, root, lines);
        assert_eq!(
            session.analysis_notice(),
            Some(&AnalysisNotice::LineTruncated(2))
        );
        let game = session.game();
        let d4 = game.children(root).expect("children")[1];
        assert_eq!(game.comment_before(d4).expect("node"), Some("t"));
        assert_eq!(game.children(d4).expect("children").len(), 1);
    }

    #[test]
    fn a_line_whose_first_move_is_illegal_adds_nothing() {
        let (mut session, nodes) = italian();
        let depth = session.undo_depth();
        // After 1.e4 the same move is illegal: use it from the node `e4`.
        let illegal = pv(session.game(), nodes[0], &["e2e4"]);
        let lines = vec![EngineLineToAdd {
            pv: illegal,
            comment: None,
        }];
        add(&mut session, nodes[1], lines);
        assert_eq!(
            session.analysis_notice(),
            Some(&AnalysisNotice::LineTruncated(0))
        );
        assert_eq!(session.undo_depth(), depth);
    }

    #[test]
    fn a_stale_node_gives_the_stale_notice_and_an_empty_line_nothing() {
        let (mut session, nodes) = italian();
        let mut other = Game::new();
        let other_root = other.root();
        let stale = play(
            &mut other,
            other_root,
            &["e2e4", "e7e5", "g1f3", "b8c6", "f1c4"],
        );
        let lines = vec![EngineLineToAdd {
            pv: pv(session.game(), nodes[0], &["d2d4"]),
            comment: None,
        }];
        add(&mut session, stale, lines);
        assert_eq!(session.notice(), Some(&Notice::StaleNode));
        session.update(GameMessage::AddEngineLine {
            node: nodes[0],
            pv: Vec::new(),
            comment: None,
        });
        assert_eq!(session.notice(), None);
    }

    #[test]
    fn prepending_puts_the_text_in_front_of_the_comment() {
        let (mut session, nodes) = italian();
        let node = nodes[2];
        session.update(GameMessage::PrependComment {
            node,
            text: "20:+0.35".to_owned(),
        });
        assert_eq!(
            session.game().comment(node).expect("node"),
            Some("20:+0.35")
        );
        session.update(GameMessage::PrependComment {
            node,
            text: "[SF] 22:M2".to_owned(),
        });
        assert_eq!(
            session.game().comment(node).expect("node"),
            Some("[SF] 22:M2 20:+0.35")
        );
        // The root takes the initial comment.
        session.update(GameMessage::PrependComment {
            node: nodes[0],
            text: "x".to_owned(),
        });
        assert_eq!(session.game().initial_comment(), Some("x"));
        // One undo point each.
        session.update(GameMessage::Undo);
        assert_eq!(session.game().initial_comment(), None);
    }

    #[test]
    fn an_open_dialog_blocks_the_messages_like_any_edit() {
        let (mut session, nodes) = italian();
        session.update(GameMessage::Navigate(crate::Nav::Start));
        // The root already has a continuation: the move opens the question.
        let d4 = pv(session.game(), nodes[0], &["d2d4"])[0];
        session.update(GameMessage::PlayMove {
            mv: d4,
            options: crate::PlayOptions::default(),
        });
        assert!(session.prompt().is_some());
        let lines = vec![line(session.game(), nodes[0], &["c2c4"], "a")];
        let count = session.game().node_count();
        add(&mut session, nodes[0], lines);
        assert_eq!(session.game().node_count(), count);
        assert_eq!(session.notice(), Some(&Notice::PromptPending));
    }
}
