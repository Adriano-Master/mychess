//! Mates in the main line of ordinary games, without markers (spec M8-4,
//! D1970-D1982).
//!
//! The marker `****D` of the automatic annotation only flags the mistakes
//! of a game, so a game that ends in a mate never became a problem. This
//! module finds the positions of the main line in which the side to move
//! has a forced mate in 1 to 5 moves. It never runs the solver on every
//! position: [`scan_plies`] picks a few candidates from cheap hints, and
//! [`find_next`](super::source::find_next) classifies them within its
//! budget.
//!
//! The hints, in this order of trust:
//!
//! 1. `[%eval #N]` in the comment of a node: the engine saw a forced mate;
//! 2. the game ends in checkmate: the positions in which the winner was to
//!    move, back from the last one;
//! 3. a game without any evaluation and without a mate at the end: the last
//!    plies in which the side to move is clearly ahead in material.

use std::collections::BTreeSet;

use mychess_core::{Color, Game, Move, NodeId, Position, Role};

use super::config::TacticsConfig;

/// Nodes the solver may use for one candidate position (D1978).
pub const SCAN_NODES: u64 = 100_000;

/// Most candidates taken from one game (D1978).
pub const MAX_SCAN_CANDIDATES: usize = 12;

/// How many of the last plies of a game without evaluations are looked at
/// (D1977).
const TAIL_PLIES: usize = 10;

/// Material (in pawns) that the side to move must be ahead by for a
/// position of the tail to be tried (D1977, D1986): a rook's worth.
const MATERIAL_EDGE: i32 = 5;

/// Most positions that the tail rule offers from one game, the ones closest
/// to the end (D1986).
const TAIL_CANDIDATES: usize = 3;

/// What the `[%eval ...]` of a comment says about a mate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MateHint {
    /// The side that mates: `true` for White (`#`, `#3`), `false` for Black
    /// (`#-`, `#-3`).
    pub white_mates: bool,
    /// In how many moves, when the comment says it.
    pub moves: Option<u32>,
}

/// The mate that the `[%eval #...]` of `comment` announces, if any
/// (Scid writes the evaluation from the point of view of White; `#-2` is
/// a mate in two for Black).
pub fn mate_hint(comment: &str) -> Option<MateHint> {
    let at = comment.find("[%eval")?;
    let rest = comment[at + "[%eval".len()..].trim_start();
    let rest = rest.strip_prefix('#')?;
    let (white_mates, digits) = match rest.strip_prefix('-') {
        Some(after) => (false, after),
        None => (true, rest.strip_prefix('+').unwrap_or(rest)),
    };
    let len = digits.bytes().take_while(u8::is_ascii_digit).count();
    // `#0` is the mate already given (the side to move has none to give):
    // not a hint (D1986).
    let moves = match digits[..len].parse::<u32>() {
        Ok(0) => return None,
        Ok(n) => Some(n),
        Err(_) => None,
    };
    Some(MateHint { white_mates, moves })
}

/// Whether `comment` carries any evaluation command.
fn has_eval(comment: &str) -> bool {
    comment.contains("[%eval")
}

/// Whether `color` still has a rook or a queen.
fn has_major_piece(position: &Position, color: Color) -> bool {
    position
        .pieces()
        .any(|(_, p)| p.color == color && matches!(p.role, Role::Rook | Role::Queen))
}

/// The material of `color` in pawns (the king does not count).
fn material(position: &Position, color: Color) -> i32 {
    position
        .pieces()
        .filter(|(_, piece)| piece.color == color)
        .map(|(_, piece)| match piece.role {
            Role::Pawn => 1,
            Role::Knight | Role::Bishop => 3,
            Role::Rook => 5,
            Role::Queen => 9,
            Role::King => 0,
        })
        .sum()
}

/// The plies (positions after that many plies, from 1) of the main line of
/// a game that starts from the standard position where a forced mate for
/// the side to move is worth looking for, in ascending order and at most
/// [`MAX_SCAN_CANDIDATES`] of them.
///
/// The position after the last move is never a candidate when it is a
/// checkmate: it has no move for the attacker (D1976).
pub fn scan_plies(game: &Game, config: &TacticsConfig) -> Vec<usize> {
    let nodes: Vec<NodeId> = game.mainline().collect();
    let count = nodes.len();
    let white_starts = game.start_position().turn() == Color::White;
    let mut by_eval = BTreeSet::new();
    let mut annotated = false;
    // 1. The evaluations of the comments.
    for (index, node) in nodes.iter().enumerate() {
        let Ok(Some(comment)) = game.comment(*node) else {
            continue;
        };
        annotated |= has_eval(comment);
        let Some(hint) = mate_hint(comment) else {
            continue;
        };
        let ply = index + 1;
        // The side that starts moves after an even number of plies.
        let white_to_move = (ply % 2 == 0) == white_starts;
        let within = hint.moves.is_none_or(|n| n <= u32::from(config.mate_max));
        if hint.white_mates == white_to_move && within {
            by_eval.insert(ply);
        }
    }
    // 2. A game that ends in mate: the moves of the winner, back from it.
    let mut by_end = BTreeSet::new();
    let mut by_tail = Vec::new();
    let ends_in_mate = nodes
        .last()
        .and_then(|node| game.position_at(*node).ok())
        .is_some_and(Position::is_checkmate);
    if ends_in_mate {
        for step in 0..usize::from(config.mate_max) {
            let Some(ply) = count.checked_sub(1 + 2 * step) else {
                break;
            };
            if ply >= 1 {
                by_end.insert(ply);
            }
        }
    } else if !annotated {
        // 3. The tail of a game that nobody evaluated: the side to move is
        // a rook ahead and still has a major piece (D1986); the ones
        // closest to the end.
        for ply in (count.saturating_sub(TAIL_PLIES).max(1)..=count).rev() {
            let Some(position) = nodes
                .get(ply - 1)
                .and_then(|node| game.position_at(*node).ok())
            else {
                continue;
            };
            let side = position.turn();
            if has_major_piece(position, side)
                && material(position, side) - material(position, !side) >= MATERIAL_EDGE
            {
                by_tail.push(ply);
                if by_tail.len() == TAIL_CANDIDATES {
                    break;
                }
            }
        }
    }
    // The positions closest to a mate come first when there is no room for
    // all: the end of the game, then the evaluations nearest to it, then
    // the tail (D1986).
    let mut chosen: BTreeSet<usize> = by_end.into_iter().take(MAX_SCAN_CANDIDATES).collect();
    for ply in by_eval.into_iter().rev().chain(by_tail) {
        if chosen.len() >= MAX_SCAN_CANDIDATES {
            break;
        }
        chosen.insert(ply);
    }
    chosen.into_iter().collect()
}

/// The moves that follow `node` in the main line, if they are exactly a
/// forced mate in `moves` moves of the side to move at `position`: legal,
/// `2 * moves - 1` plies, ending in checkmate (D1980).
pub(crate) fn game_line_if_mate(
    game: &Game,
    node: NodeId,
    position: &Position,
    moves: u8,
) -> Option<Vec<Move>> {
    let wanted = usize::from(moves) * 2 - 1;
    let line: Vec<Move> = game
        .mainline_from(node)
        .ok()?
        .take(wanted)
        .filter_map(|id| game.mv(id).ok())
        .collect();
    if line.len() != wanted {
        return None;
    }
    let mut current = position.clone();
    for mv in &line {
        current.play(*mv).ok()?;
    }
    current.is_checkmate().then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(game: &mut Game, sans: &[&str], comments: &[(usize, &str)]) {
        let mut cursor = game.root();
        for (index, uci) in sans.iter().enumerate() {
            let pos = game.position_at(cursor).expect("node").clone();
            let mv = pos.parse_uci(uci).expect("uci");
            cursor = game.add_move(cursor, mv).expect("legal");
            if let Some((_, text)) = comments.iter().find(|(i, _)| *i == index + 1) {
                game.set_comment(cursor, text).expect("comment");
            }
        }
    }

    /// Fool's mate: 1. f3 e5 2. g4 Qh4#.
    const FOOLS: [&str; 4] = ["f2f3", "e7e5", "g2g4", "d8h4"];

    #[test]
    fn the_hint_reads_the_mate_for_either_side_and_its_distance() {
        let hint = |text: &str| mate_hint(text);
        assert_eq!(
            hint("[%eval #-1]"),
            Some(MateHint {
                white_mates: false,
                moves: Some(1)
            })
        );
        assert_eq!(
            hint("x [%eval #3] y"),
            Some(MateHint {
                white_mates: true,
                moves: Some(3)
            })
        );
        assert_eq!(
            hint("[%eval #]"),
            Some(MateHint {
                white_mates: true,
                moves: None
            })
        );
        assert_eq!(hint("[%eval 0.3]"), None);
        assert_eq!(hint("no command #-2"), None);
    }

    #[test]
    fn a_game_that_ends_in_mate_offers_the_positions_of_the_winner() {
        let mut game = Game::new();
        play(&mut game, &FOOLS, &[]);
        let config = TacticsConfig {
            mate_max: 2,
            ..TacticsConfig::default()
        };
        // After 3 plies Black is to move (mate in 1); after 1 ply too (mate
        // in 2 is not there, the solver will say so).
        assert_eq!(scan_plies(&game, &config), vec![1, 3]);
    }

    #[test]
    fn the_checkmate_position_itself_is_never_a_candidate() {
        let mut game = Game::new();
        play(&mut game, &FOOLS, &[(4, "[%eval #-1]")]);
        let config = TacticsConfig::default();
        assert!(!scan_plies(&game, &config).contains(&4));
    }

    #[test]
    fn an_evaluation_with_a_mate_for_the_side_to_move_makes_a_candidate() {
        // Not a mate at the end: the hint is what points at the position.
        let mut game = Game::new();
        play(&mut game, &FOOLS[..3], &[(3, "[%eval #-1]")]);
        let config = TacticsConfig::default();
        assert_eq!(scan_plies(&game, &config), vec![3]);
    }

    #[test]
    fn a_mate_for_the_other_side_or_too_long_is_not_a_hint() {
        let mut game = Game::new();
        // After ply 3 Black is to move: a mate for White does not count.
        play(&mut game, &FOOLS[..3], &[(3, "[%eval #1]")]);
        assert!(scan_plies(&game, &TacticsConfig::default()).is_empty());
        let mut long = Game::new();
        play(&mut long, &FOOLS[..3], &[(3, "[%eval #-5]")]);
        let short = TacticsConfig {
            mate_max: 3,
            ..TacticsConfig::default()
        };
        assert!(scan_plies(&long, &short).is_empty());
    }

    #[test]
    fn an_annotated_game_trusts_its_evaluations_and_skips_the_tail_rule() {
        let mut game = Game::new();
        play(&mut game, &["e2e4", "e7e5"], &[(1, "[%eval 0.2]")]);
        assert!(scan_plies(&game, &TacticsConfig::default()).is_empty());
    }

    #[test]
    fn the_game_line_is_used_only_when_it_is_that_mate() {
        let mut game = Game::new();
        play(&mut game, &FOOLS, &[]);
        let nodes: Vec<NodeId> = game.mainline().collect();
        let position = game.position_at(nodes[2]).expect("node").clone();
        assert_eq!(
            game_line_if_mate(&game, nodes[2], &position, 1).map(|l| l.len()),
            Some(1)
        );
        // A mate in two would need three plies that the game does not have.
        assert_eq!(game_line_if_mate(&game, nodes[2], &position, 2), None);
    }

    fn from_fen(fen: &str) -> Game {
        Game::from_position(Position::from_fen(fen).expect("fen"))
    }

    #[test]
    fn a_mate_already_given_is_not_a_hint() {
        assert_eq!(mate_hint("[%eval #0]"), None);
        assert_eq!(mate_hint("[%eval #-0]"), None);
        let mut game = Game::new();
        play(&mut game, &FOOLS, &[(4, "[%eval #-0]")]);
        // Only the end of the game points at Qh4#'s position.
        assert_eq!(scan_plies(&game, &TacticsConfig::default()), vec![1, 3]);
    }

    #[test]
    fn the_side_that_starts_decides_who_is_to_move_after_each_ply() {
        // Black starts: after one ply White is to move, so `#1` counts.
        let mut game = from_fen("4k3/8/8/8/8/8/7r/4K2R b - - 0 1");
        play(&mut game, &["e8d8"], &[(1, "[%eval #1]")]);
        assert_eq!(scan_plies(&game, &TacticsConfig::default()), vec![1]);
        let mut other = from_fen("4k3/8/8/8/8/8/7r/4K2R b - - 0 1");
        play(&mut other, &["e8d8"], &[(1, "[%eval #-1]")]);
        assert!(scan_plies(&other, &TacticsConfig::default()).is_empty());
    }

    #[test]
    fn too_many_evaluations_keep_the_ones_closest_to_the_end() {
        let mut game = Game::new();
        let shuffle: Vec<&str> = ["g1f3", "g8f6", "f3g1", "f6g8"]
            .iter()
            .copied()
            .cycle()
            .take(40)
            .collect();
        let comments: Vec<(usize, &str)> =
            (1..=39).step_by(2).map(|p| (p, "[%eval #-3]")).collect();
        play(&mut game, &shuffle, &comments);
        let plies = scan_plies(&game, &TacticsConfig::default());
        assert_eq!(plies.len(), MAX_SCAN_CANDIDATES);
        assert_eq!(plies.first(), Some(&17));
        assert_eq!(plies.last(), Some(&39));
    }

    #[test]
    fn the_tail_rule_wants_a_rook_ahead_and_gives_the_three_closest_positions() {
        let mut game = from_fen("4k3/8/8/8/8/8/8/R3K2R w - - 0 1");
        play(
            &mut game,
            &[
                "a1a2", "e8d8", "a2a3", "d8c8", "a3a4", "c8b8", "a4a5", "b8c8", "a5a6", "c8d8",
            ],
            &[],
        );
        assert_eq!(scan_plies(&game, &TacticsConfig::default()), vec![6, 8, 10]);
        // Without a major piece the edge is not enough.
        let mut minor = from_fen("4k3/8/8/8/8/8/8/2B1KB1B w - - 0 1");
        play(&mut minor, &["c1d2", "e8d8", "d2c3", "d8c8"], &[]);
        assert!(scan_plies(&minor, &TacticsConfig::default()).is_empty());
    }
}
