//! `build_notation` against the PGN writer (spec rule 1-13, D206): the flat
//! text of the document is the writer's movetext, minus the braces.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::unreadable_literal,
    clippy::assigning_clones,
    clippy::manual_assert_eq,
    clippy::format_collect,
    clippy::unchecked_time_subtraction,
    clippy::field_reassign_with_default,
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_pass_by_value
)]

mod m2_4_common;

use m2_4_common::*;
use mychess_pgn::write::{Header, LineWidth, NagOutput, WriteOptions, game_to_string};
use proptest::prelude::*;

/// A random tree: `(parent index, move index, decoration)` per step.
fn random_game(ops: &[(usize, usize, u8)]) -> Game {
    let mut game = Game::new();
    let mut nodes = vec![game.root()];
    for (i, &(a, b, c)) in ops.iter().enumerate() {
        let parent = nodes[a % nodes.len()];
        let moves = game.position_at(parent).unwrap().legal_moves();
        if moves.is_empty() {
            continue;
        }
        let Ok(id) = game.add_move(parent, moves[b % moves.len()]) else {
            continue;
        };
        if !nodes.contains(&id) {
            nodes.push(id);
        }
        if c % 4 == 0 {
            game.set_comment(id, &format!("note {i}")).unwrap();
        }
        if c % 5 == 1 {
            game.add_nag(id, nag(u16::from(c % 6) + 1)).unwrap();
        }
        if c % 7 == 2 {
            game.add_nag(id, nag(u16::from(c % 8) + 10)).unwrap();
        }
        if c % 6 == 3 {
            game.set_comment_before(id, &format!("before {i}")).unwrap();
        }
        if i == 2 && c % 2 == 0 {
            game.set_initial_comment("start");
        }
    }
    game
}

fn norm(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn writer_movetext(game: &Game, comments: bool, variations: bool) -> String {
    let opts = WriteOptions::default()
        .with_header(Header::Pgn)
        .with_nags(if comments {
            NagOutput::Numeric
        } else {
            NagOutput::None
        })
        .with_comments(comments)
        .with_variations(variations)
        .with_move_number_space(false)
        .with_width(LineWidth::Unlimited);
    // `-comments 0` of the Scid hides the NAGs too (rule 5).
    let pgn = game_to_string(game, &opts).unwrap();
    let movetext = pgn.rsplit_once("\n\n").map_or(pgn.as_str(), |(_, m)| m);
    norm(&movetext.replace(['{', '}'], ""))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn flat_text_equals_the_writer_movetext(
        ops in prop::collection::vec((0usize..64, 0usize..64, any::<u8>()), 1..70),
        layout in prop_oneof![Just(NotationLayout::Inline), Just(NotationLayout::Indented)],
        comments in any::<bool>(),
        variations in any::<bool>(),
    ) {
        let game = random_game(&ops);
        let o = opts(|o| {
            o.layout = layout;
            o.symbolic_nags = false;
            o.move_number_space = false;
            o.show_comments = comments;
            o.show_variations = variations;
        });
        let doc = build_notation(&game, &o, 1);
        prop_assert_eq!(norm(&doc.flat_text()), writer_movetext(&game, comments, variations));
    }

    /// Parentheses are balanced, every move node exists once, and the move
    /// count is the number of nodes shown.
    #[test]
    fn structure_invariants(
        ops in prop::collection::vec((0usize..64, 0usize..64, any::<u8>()), 1..70),
        indented in any::<bool>(),
    ) {
        let game = random_game(&ops);
        let layout = if indented { NotationLayout::Indented } else { NotationLayout::Inline };
        let doc = build_notation(&game, &opts(|o| o.layout = layout), 7);
        let text = doc.flat_text();
        prop_assert_eq!(text.matches('(').count(), text.matches(')').count());
        let mut seen = std::collections::HashSet::new();
        let mut moves = 0usize;
        for i in doc.blocks.iter().flat_map(|b| &b.inlines) {
            if let Inline::Move { node, .. } = i {
                prop_assert!(seen.insert(*node), "node shown twice");
                prop_assert!(game.contains(*node));
                moves += 1;
            }
        }
        prop_assert_eq!(moves, game.node_count() - 1);
        prop_assert!(doc.blocks.iter().all(|b| b.rev == 7 && b.indent <= MAX_INDENT));
    }

    /// Building twice gives the same document (no hidden state), and the
    /// cursor-free build is what `keep_revisions` keeps whole.
    #[test]
    fn build_is_deterministic_and_keep_revisions_is_idempotent(
        ops in prop::collection::vec((0usize..64, 0usize..64, any::<u8>()), 1..40),
    ) {
        let game = random_game(&ops);
        let o = NotationOptions::default();
        let a = build_notation(&game, &o, 1);
        let mut b = build_notation(&game, &o, 2);
        b.keep_revisions(&a);
        prop_assert_eq!(&a.blocks, &b.blocks);
    }
}
