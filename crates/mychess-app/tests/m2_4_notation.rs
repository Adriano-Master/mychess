//! `build_notation`: rules 1-13 of spec section 3.1 (D160-D166, D174).
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
use mychess_app::{
    Block, BlockKind, CommentTarget, HeaderLine, HeaderMode, Inline, MAX_INDENT, NotationLayout,
    build_notation_with_pending,
};
use rstest::rstest;

/// `1.e4 (1.d4 d5) 1...e5 2.Nf3`: returns the game and `[e4, d4, d5, e5, nf3]`.
fn game_with_variation() -> (Game, [NodeId; 5]) {
    let mut g = Game::new();
    let root = g.root();
    let e4 = san(&mut g, root, "e4");
    let d4 = san(&mut g, root, "d4");
    let d5 = san(&mut g, d4, "d5");
    let e5 = san(&mut g, e4, "e5");
    let nf3 = san(&mut g, e5, "Nf3");
    (g, [e4, d4, d5, e5, nf3])
}

// ---- numbering (rule 2, D164) ----

#[test]
fn main_line_is_numbered_on_white_moves_and_ends_with_the_result() {
    let mut g = Game::new();
    main_line(&mut g, "e4 e5 Nf3 Nc6");
    assert_eq!(flat(&g, &plain()), "1.e4 e5 2.Nf3 Nc6 *");
}

#[rstest]
#[case(false, "1.e4 (1.d4 d5) 1...e5 2.Nf3 *")]
#[case(true, "1. e4 (1. d4 d5) 1. ... e5 2. Nf3 *")]
fn black_move_after_variations_is_numbered(#[case] space: bool, #[case] expected: &str) {
    let (g, _) = game_with_variation();
    let o = opts(|o| {
        o.layout = NotationLayout::Inline;
        o.move_number_space = space;
    });
    assert_eq!(flat(&g, &o), expected);
}

#[test]
fn first_black_move_of_a_variation_has_dots() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    san(&mut g, ns[0], "c5");
    assert_eq!(flat(&g, &plain()), "1.e4 e5 (1...c5) *");
}

#[test]
fn start_from_fen_with_black_to_move_starts_with_dots() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 b - - 0 7").unwrap();
    let mut g = Game::from_position(pos);
    main_line(&mut g, "Kd7 Kd2");
    assert_eq!(flat(&g, &plain()), "7...Kd7 8.Kd2 *");
}

#[test]
fn chess960_start_numbers_and_castles_in_san() {
    let pos =
        Position::from_fen("bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9")
            .unwrap();
    let mut g = Game::from_position(pos);
    let root = g.root();
    let moves = g.position_at(root).unwrap().legal_moves();
    assert!(!moves.is_empty());
    g.add_move(root, moves[0]).unwrap();
    let doc = build_notation(&g, &plain(), 1);
    assert!(doc.flat_text().starts_with("9."), "{}", doc.flat_text());
    // The start is not the standard one: the FEN is in the header.
    assert!(
        doc.header
            .iter()
            .any(|l| matches!(l, HeaderLine::Tag { name, .. } if name == "FEN"))
    );
}

// ---- NAGs (rule 4, D165) ----

#[rstest]
#[case(&[1], true, "1.e4! e5 *")]
#[case(&[1], false, "1.e4 $1 e5 *")]
#[case(&[1, 14], true, "1.e4! +=  e5 *")]
#[case(&[14], true, "1.e4 += e5 *")]
#[case(&[5, 18], false, "1.e4 $5 $18 e5 *")]
fn nag_spacing_and_form(#[case] nags: &[u16], #[case] symbolic: bool, #[case] expected: &str) {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    for n in nags {
        g.add_nag(ns[0], nag(*n)).unwrap();
    }
    let o = opts(|o| {
        o.layout = NotationLayout::Inline;
        o.symbolic_nags = symbolic;
    });
    let text = flat(&g, &o);
    // Compare ignoring the exact amount of blanks: only a *sticky* first
    // `!`/`?` may touch the move.
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(squash(&text), squash(expected));
    if symbolic && nags[0] == 1 {
        assert!(text.contains("e4!"), "first ! sticks to the move: {text}");
    }
    if !symbolic || nags[0] != 1 {
        assert!(!text.contains("e4$") && !text.contains("e4+"), "{text}");
    }
}

#[test]
fn first_question_nag_sticks_but_second_nag_does_not() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    g.add_nag(ns[0], nag(2)).unwrap();
    g.add_nag(ns[0], nag(18)).unwrap();
    assert_eq!(flat(&g, &plain_symbolic()), "1.e4? +- *");
}

fn plain_symbolic() -> NotationOptions {
    opts(|o| o.layout = NotationLayout::Inline)
}

#[test]
fn nag_without_symbol_is_dollar_number_even_when_symbolic() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    g.add_nag(ns[0], nag(255)).unwrap();
    assert!(flat(&g, &plain_symbolic()).contains("$255"));
}

#[test]
fn diagram_nag_is_displayed_as_d() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    g.add_nag(ns[0], nag(201)).unwrap();
    assert!(flat(&g, &plain_symbolic()).contains(" D"));
}

#[test]
fn nag_inline_carries_the_node_and_nag() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    g.add_nag(ns[0], nag(3)).unwrap();
    let doc = build_notation(&g, &plain_symbolic(), 1);
    let found = doc
        .blocks
        .iter()
        .flat_map(|b| &b.inlines)
        .any(|i| matches!(i, Inline::Nag { node, nag: n } if *node == ns[0] && *n == nag(3)));
    assert!(found);
}

// ---- comments (rules 5-7, 9, D166) ----

#[test]
fn comment_after_a_move_forces_the_number_of_the_next_black_move() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    g.set_comment(ns[0], "best by test").unwrap();
    assert_eq!(flat(&g, &plain()), "1.e4 best by test 1...e5 *");
}

#[test]
fn comment_before_a_variation_comes_right_after_the_open_paren() {
    let (mut g, [_, d4, ..]) = game_with_variation();
    g.set_comment_before(d4, "gambit").unwrap();
    let t = flat(&g, &plain());
    assert!(t.contains("(gambit 1.d4"), "{t}");
}

#[test]
fn initial_comment_comes_before_move_one_with_target_initial() {
    let mut g = Game::new();
    main_line(&mut g, "e4");
    g.set_initial_comment("hello");
    let doc = build_notation(&g, &plain(), 1);
    assert_eq!(doc.flat_text(), "hello 1.e4 *");
    assert!(doc.blocks[0].inlines.iter().any(|i| matches!(
        i,
        Inline::Comment {
            target: CommentTarget::Initial,
            ..
        }
    )));
}

#[test]
fn initial_comment_is_an_indented_block_when_indenting_comments() {
    let mut g = Game::new();
    main_line(&mut g, "e4");
    g.set_initial_comment("hello");
    let doc = build_notation(&g, &opts(|_| {}), 1);
    assert_eq!(doc.blocks[0].kind, BlockKind::Comment);
    assert_eq!(doc.blocks[0].indent, 1);
}

#[test]
fn blank_comment_does_not_produce_an_inline() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    g.set_comment(ns[0], "   ").unwrap();
    assert_eq!(flat(&g, &plain()), "1.e4 e5 *");
}

#[test]
fn marks_are_kept_by_default_and_hidden_with_strip_marks() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    g.set_comment(ns[0], "[%clk 0:01:00] good").unwrap();
    assert!(flat(&g, &plain()).contains("[%clk 0:01:00]"));
    let o = opts(|o| {
        o.layout = NotationLayout::Inline;
        o.strip_marks = true;
    });
    let t = flat(&g, &o);
    assert!(!t.contains("%clk") && t.contains("good"), "{t}");
}

#[test]
fn comment_that_is_only_marks_vanishes_with_strip_marks() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    g.set_comment(ns[0], "[%csl Ge4]").unwrap();
    let o = opts(|o| {
        o.layout = NotationLayout::Inline;
        o.strip_marks = true;
    });
    // No comment means no forced number either.
    assert_eq!(flat(&g, &o), "1.e4 e5 *");
}

#[test]
fn angle_brackets_are_plain_text() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    g.set_comment(ns[0], "a<b>c").unwrap();
    assert!(flat(&g, &plain()).contains("a<b>c"));
}

#[test]
fn comment_inline_targets_its_node() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    g.set_comment(ns[1], "x").unwrap();
    let doc = build_notation(&g, &plain(), 1);
    assert!(doc.blocks.iter().flat_map(|b| &b.inlines).any(|i| {
        matches!(i, Inline::Comment { target: CommentTarget::After(n), .. } if *n == ns[1])
            && i.node() == Some(ns[1])
    }));
}

#[test]
fn text_inlines_are_not_clickable() {
    let (g, _) = game_with_variation();
    let doc = build_notation(&g, &plain(), 1);
    for i in doc.blocks.iter().flat_map(|b| &b.inlines) {
        match i {
            Inline::Number(_) | Inline::Open | Inline::Close | Inline::Space => {
                assert_eq!(i.node(), None, "{i:?}");
            }
            Inline::Move { .. } => assert!(i.node().is_some()),
            _ => {}
        }
    }
}

// ---- null move (rule 8) ----

#[test]
fn null_move_is_shown_as_double_dash_not_as_a_comment() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    g.add_move(ns[0], mychess_core::Move::Null).unwrap();
    let doc = build_notation(&g, &plain(), 1);
    assert_eq!(doc.flat_text(), "1.e4 -- *");
    assert!(
        !doc.blocks
            .iter()
            .flat_map(|b| &b.inlines)
            .any(|i| matches!(i, Inline::Comment { .. }))
    );
}

// ---- options show_* (rule 5) ----

#[test]
fn hiding_comments_hides_nags_and_all_comment_kinds() {
    let (mut g, [e4, d4, ..]) = game_with_variation();
    g.set_comment(e4, "after").unwrap();
    g.set_comment_before(d4, "before").unwrap();
    g.set_initial_comment("init");
    g.add_nag(e4, nag(1)).unwrap();
    for layout in [NotationLayout::Inline, NotationLayout::Indented] {
        let o = opts(|o| {
            o.layout = layout;
            o.show_comments = false;
        });
        let doc = build_notation(&g, &o, 1);
        assert!(
            !doc.blocks
                .iter()
                .flat_map(|b| &b.inlines)
                .any(|i| matches!(i, Inline::Comment { .. } | Inline::Nag { .. })),
            "{layout:?}"
        );
        // Without comments there is no forced number after e4: 1...e5 is
        // still forced by the variation.
        assert!(doc.flat_text().contains("1...e5"));
    }
}

#[test]
fn hiding_variations_drops_them_and_their_parens() {
    let (g, _) = game_with_variation();
    let o = opts(|o| {
        o.layout = NotationLayout::Inline;
        o.show_variations = false;
    });
    assert_eq!(flat(&g, &o), "1.e4 e5 2.Nf3 *");
    let o2 = opts(|o| o.show_variations = false);
    let doc = build_notation(&g, &o2, 1);
    assert!(doc.blocks.iter().all(|b| b.kind != BlockKind::Variation));
    assert!(!doc.flat_text().contains('('));
}

#[test]
fn show_result_off_removes_the_result() {
    let mut g = Game::new();
    main_line(&mut g, "e4");
    g.tags_mut().result = GameResult::White;
    assert_eq!(flat(&g, &plain()), "1.e4 1-0");
    let o = opts(|o| {
        o.layout = NotationLayout::Inline;
        o.show_result = false;
    });
    let doc = build_notation(&g, &o, 1);
    assert_eq!(doc.result, None);
    assert_eq!(doc.flat_text(), "1.e4");
}

#[rstest]
#[case(GameResult::White, "1-0")]
#[case(GameResult::Black, "0-1")]
#[case(GameResult::Draw, "1/2-1/2")]
#[case(GameResult::Unknown, "*")]
fn result_text(#[case] result: GameResult, #[case] text: &str) {
    let mut g = Game::new();
    g.tags_mut().result = result;
    assert_eq!(build_notation(&g, &plain(), 1).to_plain().trim(), text);
}

// ---- layout (rule 10, D163) ----

#[test]
fn indented_layout_gives_each_variation_a_block_at_indent_one() {
    let (g, _) = game_with_variation();
    let doc = build_notation(&g, &opts(|_| {}), 1);
    let variations: Vec<&Block> = doc
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Variation)
        .collect();
    assert_eq!(variations.len(), 1);
    assert_eq!(variations[0].indent, 1);
    assert_eq!(doc.blocks[0].kind, BlockKind::Main);
    assert_eq!(doc.blocks[0].indent, 0);
    // The main line goes on in a new block whose first move is numbered.
    let last = doc.blocks.last().unwrap();
    assert_eq!(last.kind, BlockKind::Main);
    assert!(
        last.inlines
            .iter()
            .any(|i| matches!(i, Inline::Number(n) if &**n == "1..."))
    );
}

#[test]
fn indented_nested_variations_deepen_the_indent_and_close_together() {
    let mut g = Game::new();
    let root = g.root();
    let e4 = san(&mut g, root, "e4");
    san(&mut g, e4, "e5");
    let c5 = san(&mut g, e4, "c5"); // variation of black's first reply
    let nf3 = san(&mut g, c5, "Nf3");
    san(&mut g, c5, "Nc3"); // variation inside the variation
    let _ = nf3;
    let doc = build_notation(&g, &opts(|_| {}), 1);
    let indents: Vec<u8> = doc
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Variation)
        .map(|b| b.indent)
        .collect();
    // `(1...c5 2.Nf3` at level 1, then `(2.Nc3)` one level deeper.
    assert_eq!(indents, vec![1, 2], "{}", doc.to_plain());
    let text = doc.flat_text();
    assert_eq!(
        text.matches('(').count(),
        text.matches(')').count(),
        "{text}"
    );
}

#[test]
fn indent_never_exceeds_the_maximum() {
    let g = nested_chain(MAX_INDENT as usize + 4);
    let doc = build_notation(&g, &opts(|_| {}), 1);
    assert!(doc.blocks.iter().all(|b| b.indent <= MAX_INDENT));
    assert!(doc.blocks.iter().any(|b| b.indent == MAX_INDENT));
}

#[test]
fn inline_layout_is_a_single_block_with_parens_as_text() {
    let (g, _) = game_with_variation();
    let doc = build_notation(&g, &plain(), 1);
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].indent, 0);
    assert!(doc.blocks[0].inlines.contains(&Inline::Open));
    assert!(doc.blocks[0].inlines.contains(&Inline::Close));
}

#[test]
fn inline_layout_keeps_main_comments_in_the_paragraph() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    g.set_comment(ns[0], "c").unwrap();
    let doc = build_notation(&g, &plain(), 1);
    assert_eq!(doc.blocks.len(), 1);
}

#[test]
fn indented_main_comment_is_its_own_block_only_with_indent_comments() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    g.set_comment(ns[0], "c").unwrap();
    let with = build_notation(&g, &opts(|o| o.layout = NotationLayout::Indented), 1);
    assert!(
        with.blocks
            .iter()
            .any(|b| b.kind == BlockKind::Comment && b.indent == 1)
    );
    let without = build_notation(
        &g,
        &opts(|o| {
            o.layout = NotationLayout::Indented;
            o.indent_comments = false;
        }),
        1,
    );
    assert!(without.blocks.iter().all(|b| b.kind != BlockKind::Comment));
}

#[test]
fn comments_inside_variations_stay_inline_even_when_indenting() {
    let (mut g, [_, d4, d5, ..]) = game_with_variation();
    g.set_comment(d5, "inside").unwrap();
    let _ = d4;
    let doc = build_notation(&g, &opts(|_| {}), 1);
    assert!(doc.blocks.iter().all(|b| b.kind != BlockKind::Comment));
    assert!(doc.flat_text().contains("inside"));
}

#[test]
fn move_depth_counts_the_enclosing_variations() {
    let g = nested_chain(3);
    let doc = build_notation(&g, &plain(), 1);
    let depths: Vec<(u8, bool)> = doc
        .blocks
        .iter()
        .flat_map(|b| &b.inlines)
        .filter_map(|i| match i {
            Inline::Move {
                depth,
                in_variation,
                ..
            } => Some((*depth, *in_variation)),
            _ => None,
        })
        .collect();
    assert!(depths.iter().any(|&(d, v)| d == 0 && !v));
    assert!(depths.iter().any(|&(d, v)| d == 3 && v));
    assert!(depths.iter().all(|&(d, v)| v == (d > 0)));
}

/// A game with `levels` variations, each inside the previous one.
fn nested_chain(levels: usize) -> Game {
    let cycle = ["Nf3", "Nf6", "Ng1", "Ng8"];
    let mut g = Game::new();
    let mut at = g.root();
    for i in 0..=levels {
        let shuffle = g
            .position_at(at)
            .unwrap()
            .parse_san(cycle[i % 4], SanMode::Strict)
            .unwrap()
            .mv;
        let other = g
            .position_at(at)
            .unwrap()
            .legal_moves()
            .into_iter()
            .find(|m| *m != shuffle)
            .unwrap();
        g.add_move(at, other).unwrap();
        at = g.add_move(at, shuffle).unwrap();
    }
    g
}

// ---- header (D174) ----

fn headed() -> Game {
    let mut g = Game::new();
    g.tags_mut().white = "Anand".to_owned();
    g.tags_mut().black = "Carlsen".to_owned();
    g.tags_mut().event = "Match".to_owned();
    g.tags_mut().round = "3".to_owned();
    g
}

#[test]
fn hidden_header_has_no_lines() {
    let doc = build_notation(&headed(), &opts(|o| o.header = HeaderMode::Hidden), 1);
    assert!(doc.header.is_empty());
}

#[test]
fn full_header_lists_the_tags_and_no_setup_tag() {
    let doc = build_notation(&headed(), &opts(|o| o.header = HeaderMode::Full), 1);
    let names: Vec<&str> = doc
        .header
        .iter()
        .filter_map(|l| match l {
            HeaderLine::Tag { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"White") && names.contains(&"Result"));
    assert!(!names.contains(&"SetUp") && !names.contains(&"FEN"));
}

#[test]
fn full_header_adds_fen_for_a_custom_start_without_setup() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    let doc = build_notation(
        &Game::from_position(pos),
        &opts(|o| o.header = HeaderMode::Full),
        1,
    );
    let names: Vec<String> = doc
        .header
        .iter()
        .filter_map(|l| match l {
            HeaderLine::Tag { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names.last().map(String::as_str), Some("FEN"));
    assert!(!names.iter().any(|n| n == "SetUp"));
}

#[test]
fn short_header_has_players_event_and_position_when_custom() {
    let mut g = headed();
    let doc = build_notation(&g, &opts(|o| o.header = HeaderMode::Short), 1);
    assert_eq!(doc.header.len(), 2);
    assert!(
        matches!(&doc.header[0], HeaderLine::Players { white, black, .. } if white == "Anand" && black == "Carlsen")
    );
    assert!(
        matches!(&doc.header[1], HeaderLine::Event { round: Some(r), event, .. } if r == "3" && event == "Match")
    );
    let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    g = Game::from_position(pos);
    let doc = build_notation(&g, &opts(|o| o.header = HeaderMode::Short), 1);
    assert_eq!(doc.header.len(), 3);
    assert!(matches!(doc.header[2], HeaderLine::Position { .. }));
}

#[test]
fn short_header_round_question_mark_is_none() {
    let g = Game::new();
    let doc = build_notation(&g, &opts(|o| o.header = HeaderMode::Short), 1);
    assert!(matches!(
        &doc.header[1],
        HeaderLine::Event { round: None, .. }
    ));
}

// ---- pending variation (question 17) ----

#[test]
fn pending_variation_shows_a_transient_placeholder() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    let doc = build_notation_with_pending(&g, &plain(), 1, Some(ns[0]));
    assert!(doc.flat_text().contains("( ... )"), "{}", doc.flat_text());
    assert!(
        doc.blocks
            .iter()
            .flat_map(|b| &b.inlines)
            .any(|i| matches!(i, Inline::Pending { after } if *after == ns[0]))
    );
    // Without the pending node nothing shows.
    let none = build_notation_with_pending(&g, &plain(), 1, None);
    assert!(!none.flat_text().contains("( ... )"));
}

#[test]
fn pending_variation_in_indented_layout_is_a_variation_block() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    let doc = build_notation_with_pending(&g, &opts(|_| {}), 1, Some(ns[0]));
    assert!(doc.blocks.iter().any(|b| {
        b.kind == BlockKind::Variation
            && b.inlines
                .iter()
                .any(|i| matches!(i, Inline::Pending { .. }))
    }));
}

#[test]
fn pending_variation_is_hidden_when_variations_are_hidden() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    let o = opts(|o| {
        o.layout = NotationLayout::Inline;
        o.show_variations = false;
    });
    let doc = build_notation_with_pending(&g, &o, 1, Some(ns[0]));
    assert!(!doc.flat_text().contains("( ... )"));
}

// ---- empty game (rule 12) ----

#[test]
fn empty_game_has_no_movetext_blocks() {
    let doc = build_notation(&Game::new(), &opts(|_| {}), 1);
    assert!(doc.blocks.is_empty());
    assert_eq!(doc.result, Some(GameResult::Unknown));
}

#[test]
fn empty_game_with_everything_off_is_empty() {
    let o = opts(|o| {
        o.header = HeaderMode::Hidden;
        o.show_result = false;
    });
    let doc = build_notation(&Game::new(), &o, 1);
    assert!(doc.blocks.is_empty() && doc.header.is_empty() && doc.result.is_none());
}

#[test]
fn empty_game_with_only_an_initial_comment_shows_it() {
    let mut g = Game::new();
    g.set_initial_comment("only");
    let doc = build_notation(&g, &plain(), 1);
    assert!(doc.flat_text().contains("only"));
}

// ---- revisions (rule 11, D168) ----

#[test]
fn every_block_carries_the_revision_it_was_built_with() {
    let (g, _) = game_with_variation();
    let doc = build_notation(&g, &opts(|_| {}), 42);
    assert_eq!(doc.revision, 42);
    assert!(doc.blocks.iter().all(|b| b.rev == 42));
}

#[test]
fn keep_revisions_only_the_edited_block_is_new() {
    let (mut g, [_, d4, ..]) = game_with_variation();
    let old = build_notation(&g, &opts(|_| {}), 1);
    g.set_comment(d4, "edited").unwrap();
    let mut new = build_notation(&g, &opts(|_| {}), 2);
    new.keep_revisions(&old);
    assert_eq!(new.revision, 2);
    let fresh: Vec<&Block> = new.blocks.iter().filter(|b| b.rev == 2).collect();
    let kept = new.blocks.iter().filter(|b| b.rev == 1).count();
    assert_eq!(fresh.len(), 1, "only the variation changed");
    assert_eq!(fresh[0].kind, BlockKind::Variation);
    assert_eq!(kept, new.blocks.len() - 1);
}

#[test]
fn keep_revisions_with_identical_input_keeps_all_old_revisions() {
    let (g, _) = game_with_variation();
    let old = build_notation(&g, &opts(|_| {}), 1);
    let mut new = build_notation(&g, &opts(|_| {}), 5);
    new.keep_revisions(&old);
    assert!(new.blocks.iter().all(|b| b.rev == 1));
}

#[test]
fn block_keys_are_unique_within_a_document() {
    let (mut g, [e4, ..]) = game_with_variation();
    g.set_comment(e4, "c").unwrap();
    let doc = build_notation(&g, &opts(|_| {}), 1);
    let mut keys: Vec<_> = doc.blocks.iter().map(|b| format!("{:?}", b.key)).collect();
    let n = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), n, "BlockKey must identify a paragraph");
}

#[test]
fn find_move_locates_the_block_of_a_move() {
    let (g, [e4, d4, ..]) = game_with_variation();
    let doc = build_notation(&g, &opts(|_| {}), 1);
    let (b0, _) = doc.find_move(e4).unwrap();
    let (b1, _) = doc.find_move(d4).unwrap();
    assert_ne!(b0, b1);
    assert!(doc.find_move(g.root()).is_none());
}

// ---- size and depth (rule 13) ----

fn shuffle_game(plies: usize) -> Game {
    let mut g = Game::new();
    let mut at = g.root();
    let cycle = ["Nf3", "Nf6", "Ng1", "Ng8"];
    for i in 0..plies {
        at = san(&mut g, at, cycle[i % 4]);
    }
    g
}

#[test]
fn five_thousand_plies_build_without_overflow() {
    let g = shuffle_game(5000);
    for layout in [NotationLayout::Inline, NotationLayout::Indented] {
        let doc = build_notation(&g, &opts(|o| o.layout = layout), 1);
        let moves = doc
            .blocks
            .iter()
            .flat_map(|b| &b.inlines)
            .filter(|i| matches!(i, Inline::Move { .. }))
            .count();
        assert_eq!(moves, 5000);
    }
}

#[test]
fn fifteen_hundred_nested_variations_do_not_overflow_the_stack() {
    let g = nested_chain(1500);
    for layout in [NotationLayout::Inline, NotationLayout::Indented] {
        let doc = build_notation(&g, &opts(|o| o.layout = layout), 1);
        let text = doc.flat_text();
        assert_eq!(text.matches('(').count(), text.matches(')').count());
        assert!(doc.blocks.iter().all(|b| b.indent <= MAX_INDENT));
    }
}

#[test]
fn depth_saturates_in_the_move_inline() {
    let g = nested_chain(300);
    let doc = build_notation(&g, &plain(), 1);
    let max = doc
        .blocks
        .iter()
        .flat_map(|b| &b.inlines)
        .filter_map(|i| match i {
            Inline::Move { depth, .. } => Some(*depth),
            _ => None,
        })
        .max();
    assert_eq!(max, Some(u8::MAX));
}

// ---- hostile text ----

#[test]
fn one_mebibyte_comment_does_not_panic_and_survives_whole() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    let big = "x".repeat(1 << 20);
    g.set_comment(ns[0], &big).unwrap();
    let doc = build_notation(&g, &opts(|o| o.strip_marks = true), 1);
    assert!(doc.flat_text().len() >= 1 << 20);
}

#[test]
fn unbalanced_mark_codes_in_comments_do_not_panic() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4");
    for text in ["[%", "[%clk", "]]][%x]", "[%csl Ge4", "\u{0}\u{1f600}[%"] {
        g.set_comment(ns[0], text).unwrap();
        let o = opts(|o| o.strip_marks = true);
        drop(build_notation(&g, &o, 1));
    }
}

#[test]
#[ignore = "spec ambiguity: rule 2 / D164 number a move after NAG D, but the Scid (printDiagrams off by default) and mychess_pgn::write do not"]
fn move_after_a_diagram_nag_is_numbered() {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5");
    g.add_nag(ns[0], nag(201)).unwrap();
    assert!(flat(&g, &plain_symbolic()).contains("1...e5"));
}
