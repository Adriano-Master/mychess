//! Tests of the notation table: spec M2-5 section 3.1 (rules 1-7), the
//! oracle against the document, the times (D250) and the opening name
//! (D249).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::field_reassign_with_default
)]

use std::collections::{BTreeSet, HashSet};

use mychess_core::{Color, EcoBook, Game, GameResult, Nag, NodeId, Position, SanMode};
use proptest::prelude::*;

use super::*;

fn mv(game: &mut Game, parent: NodeId, san: &str) -> NodeId {
    let m = game
        .position_at(parent)
        .unwrap()
        .parse_san(san, SanMode::Strict)
        .unwrap()
        .mv;
    game.add_move(parent, m).unwrap()
}

fn line(game: &mut Game, parent: NodeId, sans: &str) -> Vec<NodeId> {
    let mut at = parent;
    sans.split_whitespace()
        .map(|s| {
            at = mv(game, at, s);
            at
        })
        .collect()
}

fn root_line(game: &mut Game, sans: &str) -> Vec<NodeId> {
    let root = game.root();
    line(game, root, sans)
}

fn root_mv(game: &mut Game, san: &str) -> NodeId {
    let root = game.root();
    mv(game, root, san)
}

fn nag(n: u16) -> Nag {
    Nag::new(n).unwrap()
}

fn project(game: &Game, opts: &NotationOptions) -> (NotationDoc, NotationTable) {
    let doc = build_notation(game, opts, 3);
    let table = NotationTable::from_doc(&doc, game, opts);
    (doc, table)
}

fn table_of(game: &Game) -> NotationTable {
    project(game, &NotationOptions::default()).1
}

/// Like [`table_of`], with `[%...]` commands hidden so that comments made
/// only of clocks do not take rows.
fn table_of_clean(game: &Game) -> NotationTable {
    let mut o = NotationOptions::default();
    o.strip_marks = true;
    project(game, &o).1
}

/// `(number, white SAN, black SAN)` of a move row.
fn moves_row(row: &Row) -> (u32, Option<&str>, Option<&str>) {
    match &row.content {
        RowContent::Moves {
            number,
            white,
            black,
        } => (
            *number,
            white.as_ref().map(|c| &*c.san),
            black.as_ref().map(|c| &*c.san),
        ),
        other => panic!("not a move row: {other:?}"),
    }
}

fn kinds(table: &NotationTable) -> Vec<RowKind> {
    table.rows.iter().map(|r| r.key.kind).collect()
}

fn cell(row: &Row, side: Color) -> &Cell {
    match &row.content {
        RowContent::Moves { white, black, .. } => match side {
            Color::White => white.as_ref().unwrap(),
            Color::Black => black.as_ref().unwrap(),
        },
        other => panic!("not a move row: {other:?}"),
    }
}

#[test]
fn default_options_use_the_table_layout() {
    let o = NotationOptions::default();
    assert_eq!(o.layout, NotationLayout::Table);
    assert_eq!(o.header, HeaderMode::Hidden);
    assert!(o.figurines && o.show_time_column);
    assert_eq!(
        NotationLayout::from_name("table"),
        Some(NotationLayout::Table)
    );
    assert_eq!(NotationLayout::Table.as_str(), "table");
}

#[test]
fn table_layout_builds_the_same_blocks_as_indented_with_indented_comments() {
    let (mut g, ids) = {
        let mut g = Game::new();
        let m = root_line(&mut g, "e4 e5 Nf3");
        let c5 = mv(&mut g, m[0], "c5");
        (g, vec![m[0], m[1], c5])
    };
    g.set_comment(ids[0], "king pawn").unwrap();
    let mut table = NotationOptions::default();
    table.move_number_space = true;
    table.indent_comments = false;
    let mut indented = table;
    indented.layout = NotationLayout::Indented;
    indented.move_number_space = false;
    indented.indent_comments = true;
    assert_eq!(
        build_notation(&g, &table, 1),
        build_notation(&g, &indented, 1)
    );
}

#[test]
fn simple_line_has_one_row_per_full_move_and_a_result_row() {
    let mut g = Game::new();
    root_line(&mut g, "e4 e5 Nf3");
    g.tags_mut().result = GameResult::Draw;
    let t = table_of(&g);
    assert_eq!(kinds(&t), [RowKind::Moves, RowKind::Moves, RowKind::Result]);
    assert_eq!(moves_row(&t.rows[0]), (1, Some("e4"), Some("e5")));
    assert_eq!(moves_row(&t.rows[1]), (2, Some("Nf3"), None));
    assert_eq!(t.rows[2].content, RowContent::Result(GameResult::Draw));
    assert_eq!(t.revision, 3);
    assert!(t.rows.iter().all(|r| r.rev == 3));
    assert!(!t.has_times);
}

#[test]
fn empty_game_has_only_the_result_or_nothing() {
    let g = Game::new();
    let t = table_of(&g);
    assert_eq!(kinds(&t), [RowKind::Result]);
    let mut o = NotationOptions::default();
    o.show_result = false;
    assert!(project(&g, &o).1.rows.is_empty());
}

#[test]
fn game_started_with_black_to_move_begins_with_an_empty_white_cell() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 b - - 0 7").unwrap();
    let mut g = Game::from_position(pos);
    root_line(&mut g, "Ke7 Kd1 Kd6");
    let t = table_of(&g);
    assert_eq!(moves_row(&t.rows[0]), (7, None, Some("Ke7")));
    assert_eq!(moves_row(&t.rows[1]), (8, Some("Kd1"), Some("Kd6")));
}

#[test]
fn number_and_colour_come_from_the_tree_not_from_the_text() {
    // The spaced option writes "1. e4" in the Indented text; the table is
    // the same either way, and `1...` after a comment is not a new number.
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5");
    g.set_comment(m[0], "good").unwrap();
    let mut o = NotationOptions::default();
    o.move_number_space = true;
    let (_, t) = project(&g, &o);
    assert_eq!(moves_row(&t.rows[0]), (1, Some("e4"), None));
    assert_eq!(t.rows[1].key.kind, RowKind::Comment);
    assert_eq!(moves_row(&t.rows[2]), (1, None, Some("e5")));
}

#[test]
fn variation_after_white_splits_the_row_and_black_continues_with_an_ellipsis_row() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5 Nf3");
    let d4 = root_mv(&mut g, "d4");
    let (doc, t) = project(&g, &NotationOptions::default());
    // 1.e4 (card 1.d4) e5 2.Nf3
    assert_eq!(
        kinds(&t),
        [
            RowKind::Moves,
            RowKind::Variation,
            RowKind::Moves,
            RowKind::Moves,
            RowKind::Result
        ]
    );
    assert_eq!(moves_row(&t.rows[0]), (1, Some("e4"), None));
    assert_eq!(t.rows[1].indent, 1);
    assert_eq!(moves_row(&t.rows[2]), (1, None, Some("e5")));
    assert_eq!(moves_row(&t.rows[3]), (2, Some("Nf3"), None));
    assert_eq!(
        t.find_move(&doc, m[1]),
        Some(MovePlace::Cell {
            row: 2,
            side: Color::Black
        })
    );
    assert_eq!(
        t.find_move(&doc, d4),
        // `( 1. d4`: the open parenthesis and the number come first.
        Some(MovePlace::Card { row: 1, inline: 2 })
    );
    assert_eq!(t.find_move(&doc, g.root()), None);
}

#[test]
fn variation_after_black_leaves_the_pair_whole() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5 Nf3");
    mv(&mut g, m[0], "c5");
    let t = table_of(&g);
    assert_eq!(
        kinds(&t),
        [
            RowKind::Moves,
            RowKind::Variation,
            RowKind::Moves,
            RowKind::Result
        ]
    );
    assert_eq!(moves_row(&t.rows[0]), (1, Some("e4"), Some("e5")));
    assert_eq!(moves_row(&t.rows[2]), (2, Some("Nf3"), None));
}

#[test]
fn nested_variations_become_cards_of_growing_indent() {
    // 1.e4 e5 (1...c5 2.Nf3 (2.c3) 2...d6) 2.Nf3
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5 Nf3");
    let c5 = mv(&mut g, m[0], "c5");
    let nf3 = mv(&mut g, c5, "Nf3");
    mv(&mut g, c5, "c3");
    mv(&mut g, nf3, "d6");
    let (doc, t) = project(&g, &NotationOptions::default());
    let cards: Vec<u8> = t
        .rows
        .iter()
        .filter(|r| r.key.kind == RowKind::Variation)
        .map(|r| r.indent)
        .collect();
    // The card 1...c5 2.Nf3, the sub-card 2.c3 and the continuation 2...d6.
    assert_eq!(cards, [1, 2, 1]);
    assert_eq!(t.rows[0].key.kind, RowKind::Moves);
    assert_eq!(table_moves(&doc, &t), doc_moves(&doc));
}

#[test]
fn deeper_nesting_is_clamped_to_four_card_levels() {
    // 1.e4 (1.d4 d5 (1...Nf6 2.c4 c5 (2...e6 3.Nc3 d5 (3...Bb4 4.e3 O-O (4...c5 5.Nf3 d5 (5...b6 6.Bd3 Bb7 (6...cxd4 7.exd4 d5 (7...Nc6))))))))
    let mut g = Game::new();
    let root = g.root();
    mv(&mut g, root, "e4");
    let d4 = mv(&mut g, root, "d4");
    let d5 = mv(&mut g, d4, "d5");
    let nf6 = mv(&mut g, d4, "Nf6");
    let _ = d5;
    let c4 = mv(&mut g, nf6, "c4");
    let c5 = mv(&mut g, c4, "c5");
    let e6 = mv(&mut g, c4, "e6");
    let nc3 = mv(&mut g, e6, "Nc3");
    let d5b = mv(&mut g, nc3, "d5");
    let bb4 = mv(&mut g, nc3, "Bb4");
    let e3 = mv(&mut g, bb4, "e3");
    let oo = mv(&mut g, e3, "O-O");
    let c5b = mv(&mut g, e3, "c5");
    let nf3 = mv(&mut g, c5b, "Nf3");
    let _ = (c5, d5b, oo, nf3);
    let doc = build_notation(&g, &NotationOptions::default(), 1);
    assert!(
        doc.blocks.iter().any(|b| b.indent >= 4),
        "the fixture must nest deep enough: {}",
        doc.to_plain()
    );
    let t = NotationTable::from_doc(&doc, &g, &NotationOptions::default());
    assert!(t.rows.iter().all(|r| r.indent <= MAX_CARD_INDENT));
    assert!(
        t.rows
            .iter()
            .any(|r| r.key.kind == RowKind::Variation && r.indent == MAX_CARD_INDENT)
    );
}

#[test]
fn comment_after_black_goes_under_the_row_and_the_next_move_starts_a_row() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5 Nf3");
    g.set_comment(m[1], "solid").unwrap();
    let (doc, t) = project(&g, &NotationOptions::default());
    assert_eq!(
        kinds(&t),
        [
            RowKind::Moves,
            RowKind::Comment,
            RowKind::Moves,
            RowKind::Result
        ]
    );
    assert_eq!(moves_row(&t.rows[0]), (1, Some("e4"), Some("e5")));
    assert_eq!(moves_row(&t.rows[2]), (2, Some("Nf3"), None));
    let RowContent::Comment { block } = t.rows[1].content else {
        panic!("comment row");
    };
    assert_eq!(
        doc.blocks[block].inlines,
        [Inline::Comment {
            target: CommentTarget::After(m[1]),
            text: "solid".into()
        }]
    );
    assert_eq!(t.rows[1].key.node, Some(m[1]));
}

#[test]
fn comment_after_white_leaves_black_for_an_ellipsis_row() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5");
    g.set_comment(m[0], "first").unwrap();
    let t = table_of(&g);
    assert_eq!(moves_row(&t.rows[0]), (1, Some("e4"), None));
    assert_eq!(moves_row(&t.rows[2]), (1, None, Some("e5")));
}

#[test]
fn initial_comment_is_the_first_row() {
    let mut g = Game::new();
    root_line(&mut g, "e4");
    g.set_initial_comment("the start");
    let t = table_of(&g);
    assert_eq!(t.rows[0].key.kind, RowKind::Comment);
    assert_eq!(t.rows[0].key.node, Some(g.root()));
    assert_eq!(t.rows[1].key.kind, RowKind::Moves);
}

#[test]
fn comments_inside_a_variation_stay_in_its_card() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5");
    let c5 = mv(&mut g, m[0], "c5");
    g.set_comment(c5, "sharp").unwrap();
    let (doc, t) = project(&g, &NotationOptions::default());
    assert_eq!(
        kinds(&t).iter().filter(|k| **k == RowKind::Comment).count(),
        0
    );
    let card = t
        .rows
        .iter()
        .find_map(|r| match r.content {
            RowContent::Variation { block } => Some(block),
            _ => None,
        })
        .unwrap();
    assert!(
        doc.blocks[card]
            .inlines
            .iter()
            .any(|i| matches!(i, Inline::Comment { .. }))
    );
}

#[test]
fn nags_go_to_the_cell_of_their_move() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5 Nf3");
    g.add_nag(m[0], nag(1)).unwrap();
    g.add_nag(m[1], nag(4)).unwrap();
    g.add_nag(m[1], nag(14)).unwrap();
    let t = table_of(&g);
    assert_eq!(cell(&t.rows[0], Color::White).nags, [nag(1)]);
    assert_eq!(cell(&t.rows[0], Color::Black).nags, [nag(4), nag(14)]);
    assert!(cell(&t.rows[1], Color::White).nags.is_empty());
}

#[test]
fn hiding_comments_hides_nags_but_keeps_the_times() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5");
    g.add_nag(m[0], nag(1)).unwrap();
    g.set_comment(m[0], "[%clk 0:05:00] [%emt 0:00:07] text")
        .unwrap();
    let mut o = NotationOptions::default();
    o.show_comments = false;
    let (_, t) = project(&g, &o);
    assert_eq!(kinds(&t), [RowKind::Moves, RowKind::Result]);
    let white = cell(&t.rows[0], Color::White);
    assert!(white.nags.is_empty());
    assert_eq!(white.times.clock, Some(300));
    assert_eq!(white.times.spent, Some(7));
    assert!(t.has_times);
}

#[test]
fn hiding_variations_leaves_no_cards() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5");
    mv(&mut g, m[0], "c5");
    let mut o = NotationOptions::default();
    o.show_variations = false;
    let (_, t) = project(&g, &o);
    assert_eq!(kinds(&t), [RowKind::Moves, RowKind::Result]);
}

#[test]
fn pending_variation_is_a_pending_row() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5");
    let o = NotationOptions::default();
    let doc = build_notation_with_pending(&g, &o, 1, Some(m[0]));
    let t = NotationTable::from_doc(&doc, &g, &o);
    let pending: Vec<_> = t
        .rows
        .iter()
        .filter(|r| r.key.kind == RowKind::Pending)
        .collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].content, RowContent::Pending { after: m[0] });
    assert_eq!(pending[0].key.node, Some(m[0]));
    assert_eq!(pending[0].indent, 1);
}

// Times (D250) ----------------------------------------------------------

fn clocked_game(comments: &[&str]) -> (Game, Vec<NodeId>) {
    let mut g = Game::new();
    let ids = root_line(&mut g, "e4 e5 Nf3 Nc6 Bb5");
    for (id, text) in ids.iter().zip(comments) {
        if !text.is_empty() {
            g.set_comment(*id, text).unwrap();
        }
    }
    (g, ids)
}

#[test]
fn only_emt_gives_the_exact_time_spent() {
    let (g, _) = clocked_game(&["[%emt 0:00:05]", "[%emt 0:00:12]"]);
    let t = table_of_clean(&g);
    let white = cell(&t.rows[0], Color::White);
    assert_eq!(
        white.times,
        MoveTimes {
            spent: Some(5),
            clock: None,
            spent_is_derived: false
        }
    );
    assert_eq!(cell(&t.rows[0], Color::Black).times.spent, Some(12));
    assert!(t.has_times);
}

#[test]
fn clocks_alone_give_an_estimate_from_the_previous_move_of_the_same_side() {
    let (g, _) = clocked_game(&[
        "[%clk 0:03:00]",
        "[%clk 0:03:00]",
        "[%clk 0:02:50]",
        "[%clk 0:02:45]",
        "[%clk 0:02:30]",
    ]);
    let t = table_of_clean(&g);
    // The first move of each side has nothing before it.
    assert_eq!(
        cell(&t.rows[0], Color::White).times,
        MoveTimes {
            spent: None,
            clock: Some(180),
            spent_is_derived: false
        }
    );
    assert_eq!(cell(&t.rows[0], Color::Black).times.spent, None);
    // Second moves: the difference with the same side, not the other one.
    let nf3 = cell(&t.rows[1], Color::White).times;
    assert_eq!(nf3.spent, Some(10));
    assert!(nf3.spent_is_derived);
    assert_eq!(cell(&t.rows[1], Color::Black).times.spent, Some(15));
    assert_eq!(cell(&t.rows[2], Color::White).times.spent, Some(20));
    assert!(t.has_times);
}

#[test]
fn emt_wins_over_the_clock_difference() {
    let (g, _) = clocked_game(&["[%clk 0:03:00]", "", "[%clk 0:02:50] [%emt 0:00:03]"]);
    let t = table_of_clean(&g);
    let nf3 = cell(&t.rows[1], Color::White).times;
    assert_eq!((nf3.spent, nf3.spent_is_derived), (Some(3), false));
}

#[test]
fn a_negative_difference_is_ignored() {
    let (g, _) = clocked_game(&["[%clk 0:01:00]", "", "[%clk 0:02:00]"]);
    let t = table_of_clean(&g);
    let nf3 = cell(&t.rows[1], Color::White).times;
    assert_eq!((nf3.spent, nf3.clock), (None, Some(120)));
}

#[test]
fn a_missing_previous_clock_gives_no_estimate() {
    let (g, _) = clocked_game(&["", "", "[%clk 0:02:00]"]);
    let t = table_of_clean(&g);
    assert_eq!(cell(&t.rows[1], Color::White).times.spent, None);
}

#[test]
fn no_times_in_the_game_means_no_time_column() {
    let (g, _) = clocked_game(&["", ""]);
    assert!(!table_of_clean(&g).has_times);
}

#[test]
fn time_column_off_skips_the_times() {
    let (g, _) = clocked_game(&["[%emt 0:00:05]"]);
    let mut o = NotationOptions::default();
    o.show_time_column = false;
    let (_, t) = project(&g, &o);
    assert!(!t.has_times);
    assert_eq!(cell(&t.rows[0], Color::White).times, MoveTimes::default());
}

// Revisions (D254) ------------------------------------------------------

#[test]
fn editing_a_nag_renews_only_its_row() {
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 e5 Nf3 Nc6 Bb5 a6");
    let o = NotationOptions::default();
    let doc1 = build_notation(&g, &o, 1);
    let table1 = NotationTable::from_doc(&doc1, &g, &o);

    g.add_nag(m[2], nag(1)).unwrap();
    let mut doc2 = build_notation(&g, &o, 2);
    doc2.keep_revisions(&doc1);
    let mut table2 = NotationTable::from_doc(&doc2, &g, &o);
    // The whole main block is new in the document...
    assert!(table2.rows.iter().all(|r| r.rev == 2));
    table2.keep_revisions(&table1);
    // ...but only the row of the edited move is in the table.
    let revs: Vec<u64> = table2.rows.iter().map(|r| r.rev).collect();
    assert_eq!(revs[0], 1);
    assert_eq!(revs[1], 2, "row 2.Nf3 changed");
    assert_eq!(revs[2], 1);
}

// Oracle (rule 5) -------------------------------------------------------

/// Moves in reading order of the table: cells, then the moves of the cards.
fn table_moves(doc: &NotationDoc, table: &NotationTable) -> Vec<NodeId> {
    let mut out = Vec::new();
    for row in &table.rows {
        match &row.content {
            RowContent::Moves { white, black, .. } => {
                out.extend(white.iter().chain(black.iter()).map(|c| c.node));
            }
            RowContent::Variation { block } => {
                out.extend(doc.blocks[*block].inlines.iter().filter_map(|i| match i {
                    Inline::Move { node, .. } => Some(*node),
                    _ => None,
                }));
            }
            _ => {}
        }
    }
    out
}

fn doc_moves(doc: &NotationDoc) -> Vec<NodeId> {
    doc.blocks
        .iter()
        .flat_map(|b| &b.inlines)
        .filter_map(|i| match i {
            Inline::Move { node, .. } => Some(*node),
            _ => None,
        })
        .collect()
}

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
            game.set_comment(id, &format!("note {i} [%clk 0:01:{:02}]", c % 60))
                .unwrap();
        }
        if c % 5 == 1 {
            game.add_nag(id, nag(u16::from(c % 6) + 1)).unwrap();
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// The table shows the moves of the document in the same order, with the
    /// same NAGs per node and the same comments.
    #[test]
    fn table_is_a_faithful_projection_of_the_document(
        ops in prop::collection::vec((0usize..64, 0usize..64, any::<u8>()), 1..70),
        comments in any::<bool>(),
        variations in any::<bool>(),
    ) {
        let game = random_game(&ops);
        let mut o = NotationOptions::default();
        o.show_comments = comments;
        o.show_variations = variations;
        let (doc, table) = project(&game, &o);
        prop_assert_eq!(table_moves(&doc, &table), doc_moves(&doc));

        // The NAGs of moves in cards stay in the block text; the cells carry
        // those of the main line.
        let doc_nags: BTreeSet<(NodeId, Nag)> = doc.blocks.iter()
            .filter(|b| b.kind == BlockKind::Main)
            .flat_map(|b| &b.inlines)
            .filter_map(|i| match i { Inline::Nag { node, nag } => Some((*node, *nag)), _ => None })
            .collect();
        let mut table_nags = BTreeSet::new();
        for row in &table.rows {
            if let RowContent::Moves { white, black, .. } = &row.content {
                for c in white.iter().chain(black.iter()) {
                    for n in &c.nags { table_nags.insert((c.node, *n)); }
                }
            }
        }
        prop_assert_eq!(table_nags, doc_nags);

        // Comments: the blocks the table refers to, and those inside cards,
        // are exactly the comments of the document.
        let comment_texts = |blocks: &mut dyn Iterator<Item = &Block>| -> Vec<String> {
            blocks.flat_map(|b| &b.inlines).filter_map(|i| match i {
                Inline::Comment { text, .. } => Some(text.to_string()), _ => None }).collect()
        };
        let from_doc = comment_texts(&mut doc.blocks.iter());
        let mut referred = Vec::new();
        for row in &table.rows {
            if let RowContent::Comment { block } | RowContent::Variation { block } = row.content {
                referred.push(&doc.blocks[block]);
            }
        }
        let from_table = comment_texts(&mut referred.into_iter());
        prop_assert_eq!(from_table, from_doc);
    }

    /// Row structure: a pair never holds two moves of one colour, numbers
    /// match the tree, indent is bounded, rows are keyed uniquely.
    #[test]
    fn rows_respect_the_tree(
        ops in prop::collection::vec((0usize..64, 0usize..64, any::<u8>()), 1..70),
    ) {
        let game = random_game(&ops);
        let (_, table) = project(&game, &NotationOptions::default());
        let mut keys = HashSet::new();
        for row in &table.rows {
            prop_assert!(row.indent <= MAX_CARD_INDENT.max(1));
            if let RowContent::Moves { number, white, black } = &row.content {
                prop_assert!(white.is_some() || black.is_some());
                for (c, color) in [(white, Color::White), (black, Color::Black)] {
                    if let Some(c) = c {
                        let before = game.position_at(game.parent(c.node).unwrap().unwrap()).unwrap();
                        prop_assert_eq!(before.turn(), color);
                        prop_assert_eq!(before.fullmove_number(), *number);
                    }
                }
                prop_assert!(keys.insert(row.key), "duplicate key");
            }
        }
    }
}

#[test]
fn deep_main_line_builds_without_recursion() {
    let mut g = Game::new();
    let mut at = g.root();
    for _ in 0..400 {
        at = mv(&mut g, at, "Nf3");
        at = mv(&mut g, at, "Nf6");
        at = mv(&mut g, at, "Ng1");
        at = mv(&mut g, at, "Ng8");
    }
    let t = table_of(&g);
    assert_eq!(t.rows.len(), 800 + 1);
}

// Opening along the path (D249) -----------------------------------------

const ECO_TEXT: &str = "B20 \"Sicilian Defence\" 1.e4 c5 *\nC20 \"King's Pawn Game\" 1.e4 e5 *\nB90 \"Sicilian: Najdorf\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6 *\n";

#[test]
fn opening_is_the_last_known_position_up_to_the_cursor() {
    let book = EcoBook::parse(ECO_TEXT).unwrap();
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 c5 Nf3 d5 Bb5+");
    let name = |node| opening_along_path(&g, node, Some(&book)).map(|o| o.name);
    // In the book: exact position.
    assert_eq!(name(m[1]).as_deref(), Some("Sicilian Defence"));
    // After the book: the last opening is kept.
    assert_eq!(name(m[4]).as_deref(), Some("Sicilian Defence"));
    // Before the first known position.
    assert_eq!(name(m[0]), None);
    // The root never names an opening.
    assert_eq!(name(g.root()), None);
    let found = opening_along_path(&g, m[3], Some(&book)).unwrap();
    assert_eq!(found.eco.to_string(), "B20");
}

#[test]
fn opening_follows_the_variation_path() {
    let book = EcoBook::parse(ECO_TEXT).unwrap();
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 c5");
    let e5 = mv(&mut g, m[0], "e5");
    let deeper = mv(&mut g, e5, "Nf3");
    let name = opening_along_path(&g, deeper, Some(&book)).map(|o| o.name);
    assert_eq!(name.as_deref(), Some("King's Pawn Game"));
}

#[test]
fn no_book_or_stale_cursor_gives_nothing() {
    let book = EcoBook::parse(ECO_TEXT).unwrap();
    let mut g = Game::new();
    let m = root_line(&mut g, "e4 c5");
    assert_eq!(opening_along_path(&g, m[1], None), None);
    let other = {
        let mut o = Game::new();
        let ids = root_line(&mut o, "e4 c5 Nf3 d6 d4 cxd4 Nxd4");
        ids[6]
    };
    assert_eq!(opening_along_path(&g, other, Some(&book)), None);
}

// Performance (D253) ----------------------------------------------------

/// `cargo test -p mychess-app --release -- --ignored table_perf --nocapture`.
/// Goal: at most 5 ms for 300 moves with 150 variations (the document is
/// built apart; this measures the projection with times).
#[test]
#[ignore = "timing: run with --release"]
fn table_perf_300_moves_150_variations() {
    use std::time::Instant;
    let mut game = tests::big_game();
    let main: Vec<NodeId> = game.mainline().collect();
    for (i, node) in main.into_iter().enumerate() {
        let left = 10_800usize.saturating_sub(i * 20);
        let text = format!(
            "[%clk {}:{:02}:{:02}]",
            left / 3600,
            left / 60 % 60,
            left % 60
        );
        game.set_comment(node, &text).unwrap();
    }
    let opts = NotationOptions::default();
    let doc = build_notation(&game, &opts, 1);
    let mut times = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let table = NotationTable::from_doc(&doc, &game, &opts);
        times.push(start.elapsed());
        assert!(table.rows.len() > 150);
        assert!(table.has_times);
    }
    times.sort();
    let median = times[times.len() / 2];
    println!(
        "rows={} median={median:?} min={:?} max={:?}",
        NotationTable::from_doc(&doc, &game, &opts).rows.len(),
        times[0],
        times[times.len() - 1]
    );
    if !cfg!(debug_assertions) {
        assert!(median.as_millis() < 5, "too slow: {median:?}");
    }
}
