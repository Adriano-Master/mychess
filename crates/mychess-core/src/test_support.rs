//! `proptest` strategies shared by the tests of the workspace crates.
//!
//! Available with the `test-support` feature (off by default), so that
//! `proptest` is not a dependency of normal builds:
//!
//! ```toml
//! [dev-dependencies]
//! mychess-core = { path = "../mychess-core", features = ["test-support"] }
//! ```
//!
//! All strategies only produce legal data: positions come from random legal
//! playouts, games are built through the public [`Game`] API.

use proptest::collection::vec;
use proptest::prelude::*;

use crate::{Game, Nag, NodeId, Position};

/// A Chess960 starting position number: `0..=959`.
pub fn arb_chess960_number() -> impl Strategy<Value = u16> {
    0u16..960
}

/// A starting position: the standard one or a Chess960 one, equally likely.
fn arb_start() -> impl Strategy<Value = Position> {
    prop_oneof![
        Just(Position::new()),
        arb_chess960_number().prop_map(|n| {
            Position::chess960_start(n).expect("test strategy: number is in 0..960")
        }),
    ]
}

/// Plays `choices` (each reduced modulo the number of legal moves) from
/// `start`, stopping when the game ends.
fn playout(start: Position, choices: &[u16]) -> Position {
    let mut pos = start;
    for choice in choices {
        let moves = pos.legal_moves();
        let Some(m) = moves.get(usize::from(*choice) % moves.len().max(1)) else {
            break;
        };
        match pos.after(*m) {
            Ok(next) => pos = next,
            Err(_) => break,
        }
    }
    pos
}

/// A legal position reached by a random playout (0 to 79 plies) from the
/// standard start position or from a Chess960 one. The castling mode of the
/// result is `Standard` or `Chess960` accordingly.
pub fn arb_position() -> impl Strategy<Value = Position> {
    (arb_start(), vec(any::<u16>(), 0..80)).prop_map(|(start, choices)| playout(start, &choices))
}

/// One step of the random construction of a game tree.
#[derive(Debug, Clone)]
enum Op {
    /// Play a legal move (by index) from the cursor, which moves there.
    Play(u16),
    /// Move the cursor to its parent, so the next move starts a variation.
    Back,
    /// Set a comment after the cursor's move.
    Comment(usize),
    /// Set a comment before the cursor's move.
    CommentBefore(usize),
    /// Attach a NAG to the cursor's move.
    Nag(u8),
}

const COMMENTS: [&str; 6] = [
    "a plain comment",
    "[%clk 0:03:21]",
    "[%eval 0.35] sharp",
    "[%csl Gd4][%cal Ge2e4] centre",
    "ünïcödé text",
    "two words",
];

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => any::<u16>().prop_map(Op::Play),
        2 => Just(Op::Back),
        1 => (0..COMMENTS.len()).prop_map(Op::Comment),
        1 => (0..COMMENTS.len()).prop_map(Op::CommentBefore),
        1 => (1u8..=255).prop_map(Op::Nag),
    ]
}

fn build_game(start: Position, ops: &[Op]) -> Game {
    let mut game = Game::from_position(start);
    let mut cursor: NodeId = game.root();
    for op in ops {
        match op {
            Op::Play(choice) => {
                let Ok(position) = game.position_at(cursor) else {
                    break;
                };
                let moves = position.legal_moves();
                if let Some(m) = moves.get(usize::from(*choice) % moves.len().max(1))
                    && let Ok(id) = game.add_move(cursor, *m)
                {
                    cursor = id;
                }
            }
            Op::Back => {
                if let Ok(Some(parent)) = game.parent(cursor) {
                    cursor = parent;
                }
            }
            // The root only has the initial comment; skip "before" there.
            Op::Comment(i) => drop(game.set_comment(cursor, COMMENTS[*i])),
            Op::CommentBefore(i) if cursor != game.root() => {
                drop(game.set_comment_before(cursor, COMMENTS[*i]));
            }
            Op::CommentBefore(_) => {}
            Op::Nag(n) => {
                if cursor != game.root()
                    && let Ok(nag) = Nag::new(u16::from(*n))
                {
                    drop(game.add_nag(cursor, nag));
                }
            }
        }
    }
    game
}

/// A legal game tree: a standard or Chess960 start, a main line, variations
/// (nested), comments (also before moves) and NAGs, up to about 120 steps.
/// The tags are the defaults.
pub fn arb_legal_game() -> impl Strategy<Value = Game> {
    (arb_start(), vec(arb_op(), 0..120)).prop_map(|(start, ops)| build_game(start, &ops))
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #[test]
        fn positions_are_legal_and_roundtrip(pos in arb_position()) {
            let mode = match pos.castling_mode() {
                crate::CastlingMode::Standard => crate::FenMode::Standard,
                crate::CastlingMode::Chess960 => crate::FenMode::Chess960,
            };
            let back = Position::from_fen_with(&pos.fen(), mode);
            prop_assert_eq!(back.ok(), Some(pos));
        }

        /// Auto mode reads back every X-FEN this crate writes, in either mode:
        /// the rights and the board are the same.
        #[test]
        fn positions_roundtrip_in_auto_mode(pos in arb_position()) {
            let back = Position::from_fen(&pos.fen());
            prop_assert_eq!(back.ok(), Some(pos));
        }

        #[test]
        fn games_have_consistent_nodes(game in arb_legal_game()) {
            let mut seen = 1; // the root
            for id in game.descendants(game.root()).unwrap() {
                prop_assert!(game.contains(id));
                seen += 1;
            }
            prop_assert_eq!(seen, game.node_count());
        }

        #[test]
        fn numbers_are_in_range(n in arb_chess960_number()) {
            prop_assert!(Position::chess960_start(n).is_ok());
        }
    }
}
