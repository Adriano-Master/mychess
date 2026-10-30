//! Applying the actions of a decision to the game (spec M4-6, rule 18).
//!
//! Pure functions over a [`Game`]; the state runs them inside one
//! [`GameSession::edit`](crate::GameSession) call per move, so a failed edit
//! leaves the game untouched.

use mychess_core::{Game, GameError, Move, Nag, NodeId};

use super::decision::Action;
use super::exercise::exercise_comment;

/// What applying actions did, for the final report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApplyStats {
    /// Variations created (a line that was in the game already counts 0).
    pub variations: u32,
    /// Exercises marked.
    pub exercises: u32,
}

/// Puts `text` in front of the comment of `node` (`"$text $comment"`).
fn prefix_comment(game: &mut Game, node: NodeId, text: &str) -> Result<(), GameError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    let joined = match game.comment(node)? {
        Some(old) if !old.trim().is_empty() => format!("{text} {old}"),
        _ => text.to_owned(),
    };
    game.set_comment(node, &joined)
}

/// Adds the line `pv` from `parent`, following the moves that exist.
/// The comment goes before the first new move and the NAG on the last move
/// of the line. A line that is all in the game already adds nothing.
fn add_line(
    game: &mut Game,
    parent: NodeId,
    pv: &[Move],
    comment: Option<&str>,
    end_nag: Option<Nag>,
) -> Result<bool, GameError> {
    let mut current = parent;
    let mut first_new = None;
    let mut last = None;
    for &mv in pv {
        // A move that cannot be played ends the line (a truncated or stale
        // PV); what was added stays.
        let Ok((id, created)) = game.add_move_new(current, mv) else {
            break;
        };
        if created && first_new.is_none() {
            first_new = Some(id);
        }
        current = id;
        last = Some(id);
    }
    let Some(first) = first_new else {
        return Ok(false);
    };
    if let Some(text) = comment {
        game.set_comment_before(first, text)?;
    }
    if let (Some(nag), Some(last)) = (end_nag, last) {
        game.add_nag(last, nag)?;
    }
    Ok(true)
}

/// Applies `actions` in order. A NAG on a node that has no move (the root,
/// for the diagram of a first move) is skipped.
///
/// # Errors
///
/// A stale node id gives [`GameError`]; the caller's edit funnel then
/// restores the game.
pub fn apply_actions(game: &mut Game, actions: &[Action]) -> Result<ApplyStats, GameError> {
    let mut stats = ApplyStats::default();
    for action in actions {
        match action {
            Action::Score { node, text } => prefix_comment(game, *node, text)?,
            Action::Nag { node, nag } => {
                if *node != game.root() {
                    game.add_nag(*node, *nag)?;
                }
            }
            Action::Variation {
                parent,
                pv,
                comment,
                end_nag,
            } => {
                if add_line(game, *parent, pv, comment.as_deref(), *end_nag)? {
                    stats.variations += 1;
                }
            }
            // The run resolves candidates before it applies; one that is
            // left is not an exercise.
            Action::ExerciseCandidate { .. } => {}
            Action::Exercise {
                node,
                difficulty,
                prev,
                cur,
            } => {
                prefix_comment(game, *node, &exercise_comment(*difficulty, *prev, *cur))?;
                stats.exercises += 1;
            }
            Action::EndComment { node, text } => game.append_comment(*node, text)?,
            Action::InitialComment { text } => game.append_comment(game.root(), text)?,
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(game: &mut Game, from: NodeId, uci: &str) -> NodeId {
        let mv = game
            .position_at(from)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        game.add_move(from, mv).expect("move")
    }

    fn pv(game: &Game, node: NodeId, ucis: &[&str]) -> Vec<Move> {
        let mut position = game.position_at(node).expect("position").clone();
        ucis.iter()
            .map(|u| {
                let mv = position.parse_uci(u).expect("uci");
                position.play(mv).expect("play");
                mv
            })
            .collect()
    }

    #[test]
    fn scores_go_in_front_of_the_existing_comment() {
        let mut game = Game::new();
        let e4 = {
            let root = game.root();
            play(&mut game, root, "e2e4")
        };
        game.set_comment(e4, "book").expect("comment");
        apply_actions(
            &mut game,
            &[Action::Score {
                node: e4,
                text: "+0.30".to_owned(),
            }],
        )
        .expect("apply");
        assert_eq!(game.comment(e4).expect("node"), Some("+0.30 book"));
    }

    #[test]
    fn a_variation_hangs_from_the_parent_with_comment_and_final_nag() {
        let mut game = Game::new();
        let root = game.root();
        let a3 = play(&mut game, root, "a2a3");
        let line = pv(&game, root, &["e2e4", "e7e5"]);
        let stats = apply_actions(
            &mut game,
            &[Action::Variation {
                parent: root,
                pv: line,
                comment: Some("Mate in 3".to_owned()),
                end_nag: Nag::new(10).ok(),
            }],
        )
        .expect("apply");
        assert_eq!(stats.variations, 1);
        let children = game.children(root).expect("children").to_vec();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0], a3);
        let e4 = children[1];
        assert_eq!(game.comment_before(e4).expect("node"), Some("Mate in 3"));
        let e5 = game.children(e4).expect("children")[0];
        assert_eq!(game.nags(e5).expect("node").len(), 1);
        assert!(game.nags(e4).expect("node").is_empty());
    }

    #[test]
    fn a_line_already_in_the_game_changes_nothing() {
        let mut game = Game::new();
        let root = game.root();
        let e4 = play(&mut game, root, "e2e4");
        play(&mut game, e4, "e7e5");
        let before = game.clone();
        let line = pv(&game, root, &["e2e4", "e7e5"]);
        let stats = apply_actions(
            &mut game,
            &[Action::Variation {
                parent: root,
                pv: line,
                comment: Some("x".to_owned()),
                end_nag: Nag::new(10).ok(),
            }],
        )
        .expect("apply");
        assert_eq!(stats.variations, 0);
        assert!(game.same_content(&before));
    }

    #[test]
    fn a_diagram_on_the_root_is_skipped_and_a_stale_node_is_an_error() {
        let mut game = Game::new();
        let root = game.root();
        let d = Nag::new(201).expect("nag");
        apply_actions(&mut game, &[Action::Nag { node: root, nag: d }]).expect("skipped");
        assert_eq!(game.node_count(), 1);
        let mut other = Game::new();
        let a3 = {
            let root = other.root();
            play(&mut other, root, "a2a3")
        };
        let a6 = play(&mut other, a3, "a7a6");
        let mut small = Game::new();
        let result = apply_actions(
            &mut small,
            &[Action::Score {
                node: a6,
                text: "x".to_owned(),
            }],
        );
        assert!(result.is_err());
    }

    #[test]
    fn exercises_prefix_the_comment_and_end_comments_append() {
        let mut game = Game::new();
        let e4 = {
            let root = game.root();
            play(&mut game, root, "e2e4")
        };
        apply_actions(
            &mut game,
            &[
                Action::Score {
                    node: e4,
                    text: "-1.80 / +0.30".to_owned(),
                },
                Action::Exercise {
                    node: e4,
                    difficulty: 3,
                    prev: 0.3,
                    cur: -1.8,
                },
                Action::EndComment {
                    node: e4,
                    text: "13:+0.20 1.e4".to_owned(),
                },
                Action::InitialComment {
                    text: "13:+0.20".to_owned(),
                },
            ],
        )
        .expect("apply");
        assert_eq!(
            game.comment(e4).expect("node"),
            Some("****D3 0.3->-1.8 -1.80 / +0.30 13:+0.20 1.e4")
        );
        assert_eq!(game.initial_comment(), Some("13:+0.20"));
    }
}
