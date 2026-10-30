//! Tests of the notation document: rules of spec section 3.1, the oracle
//! against the PGN writer, and robustness on deep trees.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::field_reassign_with_default
)]

use mychess_core::{Game, GameResult, Nag, NodeId, Position, SanMode};
use mychess_pgn::write::{LineWidth, NagOutput, WriteOptions, game_to_string};
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

fn nag(n: u16) -> Nag {
    Nag::new(n).unwrap()
}

/// Options without the Scid's indentation, to compare with plain PGN.
fn plain_opts() -> NotationOptions {
    NotationOptions {
        layout: NotationLayout::Inline,
        symbolic_nags: false,
        ..NotationOptions::default()
    }
}

fn text(game: &Game, opts: &NotationOptions) -> String {
    build_notation(game, opts, 1).flat_text()
}

/// `1.e4 e5 (1...c5 2.Nf3) 2.Nf3` and the nodes.
fn with_variation() -> (Game, Vec<NodeId>) {
    let mut g = Game::new();
    let root = g.root();
    let main = line(&mut g, root, "e4 e5 Nf3");
    let c5 = mv(&mut g, main[0], "c5");
    let nf3 = mv(&mut g, c5, "Nf3");
    (g, vec![main[0], main[1], main[2], c5, nf3])
}

#[test]
fn empty_game_has_no_blocks_and_shows_the_result() {
    let doc = build_notation(&Game::new(), &NotationOptions::default(), 7);
    assert!(doc.blocks.is_empty());
    assert_eq!(doc.result, Some(GameResult::Unknown));
    assert_eq!(doc.revision, 7);
    let mut opts = NotationOptions::default();
    opts.show_result = false;
    assert_eq!(build_notation(&Game::new(), &opts, 0).result, None);
}

#[test]
fn main_line_numbers_and_result() {
    let mut g = Game::new();
    let root = g.root();
    line(&mut g, root, "e4 e5 Nf3 Nc6");
    g.tags_mut().result = GameResult::Draw;
    assert_eq!(text(&g, &plain_opts()), "1.e4 e5 2.Nf3 Nc6 1/2-1/2");
    let mut spaced = plain_opts();
    spaced.move_number_space = true;
    assert_eq!(text(&g, &spaced), "1. e4 e5 2. Nf3 Nc6 1/2-1/2");
}

#[test]
fn black_number_with_space_option_follows_the_scid() {
    let (g, _) = with_variation();
    let mut opts = plain_opts();
    opts.move_number_space = true;
    assert_eq!(text(&g, &opts), "1. e4 e5 (1. ... c5 2. Nf3) 2. Nf3 *");
}

#[test]
fn indented_layout_gives_each_variation_a_block() {
    let (g, ids) = with_variation();
    let mut opts = NotationOptions::default();
    opts.symbolic_nags = false;
    let doc = build_notation(&g, &opts, 1);
    assert_eq!(doc.to_plain(), "1.e4 e5\n  (1...c5 2.Nf3)\n2.Nf3\n*\n");
    let kinds: Vec<_> = doc.blocks.iter().map(|b| (b.kind, b.indent)).collect();
    assert_eq!(
        kinds,
        [
            (BlockKind::Main, 0),
            (BlockKind::Variation, 1),
            (BlockKind::Main, 0)
        ]
    );
    assert_eq!(doc.blocks[1].key.node, Some(ids[3]));
}

#[test]
fn nested_variations_get_deeper_blocks_and_close_together() {
    let mut g = Game::new();
    let root = g.root();
    let m = line(&mut g, root, "e4 e5 Nf3");
    let c5 = mv(&mut g, m[0], "c5");
    let nf3 = mv(&mut g, c5, "Nf3");
    let d6 = mv(&mut g, nf3, "d6");
    mv(&mut g, nf3, "Nc6");
    mv(&mut g, d6, "d4");
    let doc = build_notation(&g, &plain_opts(), 1);
    assert_eq!(
        doc.flat_text(),
        "1.e4 e5 (1...c5 2.Nf3 d6 (2...Nc6) 3.d4) 2.Nf3 *"
    );
    let mut opts = NotationOptions::default();
    opts.symbolic_nags = false;
    assert_eq!(
        build_notation(&g, &opts, 1).to_plain(),
        "1.e4 e5\n  (1...c5 2.Nf3 d6\n    (2...Nc6)\n  3.d4)\n2.Nf3\n*\n"
    );
}

#[test]
fn variation_ending_in_a_subvariation_closes_on_its_block() {
    let mut g = Game::new();
    let root = g.root();
    let m = line(&mut g, root, "e4 e5");
    let c5 = mv(&mut g, m[0], "c5");
    let nf3 = mv(&mut g, c5, "Nf3");
    mv(&mut g, c5, "Nc3");
    let _ = nf3;
    let mut opts = NotationOptions::default();
    opts.symbolic_nags = false;
    let doc = build_notation(&g, &opts, 1);
    assert_eq!(
        doc.to_plain(),
        "1.e4 e5\n  (1...c5 2.Nf3\n    (2.Nc3))\n*\n"
    );
}

#[test]
fn comments_are_blocks_when_indented_and_force_a_number() {
    let mut g = Game::new();
    let root = g.root();
    let m = line(&mut g, root, "e4 e5 Nf3");
    g.set_comment(m[0], "best by test").unwrap();
    g.set_comment(m[1], "after black").unwrap();
    let mut opts = NotationOptions::default();
    opts.symbolic_nags = false;
    let doc = build_notation(&g, &opts, 1);
    assert_eq!(
        doc.to_plain(),
        "1.e4\n  best by test\n1...e5\n  after black\n2.Nf3\n*\n"
    );
    opts.indent_comments = false;
    assert_eq!(
        build_notation(&g, &opts, 1).flat_text(),
        "1.e4 best by test 1...e5 after black 2.Nf3 *"
    );
}

#[test]
fn comments_inside_variations_stay_inline() {
    let (mut g, ids) = with_variation();
    g.set_comment(ids[3], "sharp").unwrap();
    g.set_comment_before(ids[3], "alternative").unwrap();
    let mut opts = NotationOptions::default();
    opts.symbolic_nags = false;
    assert_eq!(
        build_notation(&g, &opts, 1).to_plain(),
        "1.e4 e5\n  (alternative 1...c5 sharp 2.Nf3)\n2.Nf3\n*\n"
    );
}

#[test]
fn initial_comment_comes_first() {
    let mut g = Game::new();
    let root = g.root();
    line(&mut g, root, "e4");
    g.set_initial_comment("Before it all");
    assert_eq!(text(&g, &plain_opts()), "Before it all 1.e4 *");
    let doc = build_notation(&g, &NotationOptions::default(), 1);
    assert_eq!(
        doc.blocks[0].inlines,
        [Inline::Comment {
            target: CommentTarget::Initial,
            text: "Before it all".into()
        }]
    );
    assert_eq!(doc.blocks[0].indent, 1);
}

#[test]
fn first_nag_sticks_only_when_it_starts_with_a_mark() {
    let mut g = Game::new();
    let root = g.root();
    let m = line(&mut g, root, "e4 e5");
    g.add_nag(m[0], nag(1)).unwrap();
    g.add_nag(m[0], nag(14)).unwrap();
    g.add_nag(m[1], nag(18)).unwrap();
    let mut opts = plain_opts();
    opts.symbolic_nags = true;
    assert_eq!(text(&g, &opts), "1.e4! += e5 +- *");
    opts.symbolic_nags = false;
    assert_eq!(text(&g, &opts), "1.e4 $1 $14 e5 $18 *");
}

#[test]
fn hiding_comments_hides_nags_too() {
    let mut g = Game::new();
    let root = g.root();
    let m = line(&mut g, root, "e4 e5");
    g.add_nag(m[0], nag(1)).unwrap();
    g.set_comment(m[0], "good").unwrap();
    let mut opts = plain_opts();
    opts.show_comments = false;
    assert_eq!(text(&g, &opts), "1.e4 e5 *");
}

#[test]
fn hiding_variations_removes_them() {
    let (g, _) = with_variation();
    let mut opts = plain_opts();
    opts.show_variations = false;
    assert_eq!(text(&g, &opts), "1.e4 e5 2.Nf3 *");
}

#[test]
fn strip_marks_drops_comments_that_become_blank() {
    let mut g = Game::new();
    let root = g.root();
    let m = line(&mut g, root, "e4 e5");
    g.set_comment(m[0], "[%clk 0:05:00]").unwrap();
    g.set_comment(m[1], "[%clk 0:04:00] fine").unwrap();
    let mut opts = plain_opts();
    assert_eq!(
        text(&g, &opts),
        "1.e4 [%clk 0:05:00] 1...e5 [%clk 0:04:00] fine *"
    );
    opts.strip_marks = true;
    assert_eq!(text(&g, &opts), "1.e4 e5 fine *");
}

#[test]
fn null_move_is_shown_as_dashes() {
    let mut g = Game::new();
    let root = g.root();
    let e4 = mv(&mut g, root, "e4");
    g.add_move(e4, mychess_core::Move::Null).unwrap();
    assert_eq!(text(&g, &plain_opts()), "1.e4 -- *");
}

#[test]
fn fen_start_with_black_to_move_begins_with_dots() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 b - - 0 7").unwrap();
    let mut g = Game::from_position(pos);
    let root = g.root();
    line(&mut g, root, "Kd7 e4");
    assert_eq!(text(&g, &plain_opts()), "7...Kd7 8.e4 *");
}

#[test]
fn every_move_and_comment_carries_its_target() {
    let (mut g, ids) = with_variation();
    g.set_comment(ids[0], "c").unwrap();
    let doc = build_notation(&g, &plain_opts(), 1);
    let moves: Vec<NodeId> = doc
        .blocks
        .iter()
        .flat_map(|b| &b.inlines)
        .filter_map(|i| match i {
            Inline::Move { node, .. } => Some(*node),
            _ => None,
        })
        .collect();
    assert_eq!(moves, [ids[0], ids[1], ids[3], ids[4], ids[2]]);
    assert!(doc.find_move(ids[4]).is_some());
    let comment = doc
        .blocks
        .iter()
        .flat_map(|b| &b.inlines)
        .find(|i| matches!(i, Inline::Comment { .. }))
        .unwrap();
    assert_eq!(comment.node(), Some(ids[0]));
    assert_eq!(Inline::Space.node(), None);
}

#[test]
fn move_depth_counts_enclosing_variations() {
    let (g, ids) = with_variation();
    let doc = build_notation(&g, &plain_opts(), 1);
    let (b, i) = doc.find_move(ids[3]).unwrap();
    match &doc.blocks[b].inlines[i] {
        Inline::Move {
            depth,
            in_variation,
            ..
        } => assert_eq!((*depth, *in_variation), (1, true)),
        other => panic!("not a move: {other:?}"),
    }
}

#[test]
fn pending_variation_shows_a_placeholder_after_the_alternatives() {
    let (g, ids) = with_variation();
    // Armed at e4's node: an alternative to `e5`.
    let doc = build_notation_with_pending(&g, &plain_opts(), 1, Some(ids[0]));
    assert_eq!(doc.flat_text(), "1.e4 e5 (1...c5 2.Nf3) ( ... ) 2.Nf3 *");
    // Armed at the root: an alternative to `e4`, with no variation yet.
    let root = g.root();
    let doc = build_notation_with_pending(&g, &plain_opts(), 1, Some(root));
    assert_eq!(
        doc.flat_text(),
        "1.e4 ( ... ) 1...e5 (1...c5 2.Nf3) 2.Nf3 *"
    );
    let mut opts = NotationOptions::default();
    opts.symbolic_nags = false;
    let indented = build_notation_with_pending(&g, &opts, 1, Some(ids[0]));
    assert_eq!(
        indented.to_plain(),
        "1.e4 e5\n  (1...c5 2.Nf3)\n  ( ... )\n2.Nf3\n*\n"
    );
    // Hidden with the variations.
    let mut opts = plain_opts();
    opts.show_variations = false;
    let doc = build_notation_with_pending(&g, &opts, 1, Some(ids[0]));
    assert_eq!(doc.flat_text(), "1.e4 e5 2.Nf3 *");
}

#[test]
fn keep_revisions_marks_only_changed_blocks() {
    let (mut g, ids) = with_variation();
    let opts = NotationOptions::default();
    let before = build_notation(&g, &opts, 1);
    g.set_comment(ids[4], "new").unwrap();
    let mut after = build_notation(&g, &opts, 2);
    after.keep_revisions(&before);
    let revs: Vec<u64> = after.blocks.iter().map(|b| b.rev).collect();
    assert_eq!(revs, [1, 2, 1]);
    assert_eq!(after.revision, 2);
}

#[test]
fn header_modes() {
    let mut g = Game::new();
    g.tags_mut().white = "Anand".to_owned();
    g.tags_mut().round = "3".to_owned();
    let mut opts = NotationOptions::default();
    opts.header = HeaderMode::Hidden;
    assert!(build_notation(&g, &opts, 0).header.is_empty());
    opts.header = HeaderMode::Full;
    let full = build_notation(&g, &opts, 0).header;
    assert!(full.contains(&HeaderLine::Tag {
        name: "White".to_owned(),
        value: "Anand".to_owned()
    }));
    assert!(
        !full
            .iter()
            .any(|l| matches!(l, HeaderLine::Tag { name, .. } if name == "FEN"))
    );
    opts.header = HeaderMode::Short;
    let short = build_notation(&g, &opts, 0).header;
    assert_eq!(short.len(), 2);
    assert!(matches!(&short[1], HeaderLine::Event { round: Some(r), .. } if r == "3"));
    let fen = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    let custom = Game::from_position(fen);
    opts.header = HeaderMode::Short;
    assert_eq!(build_notation(&custom, &opts, 0).header.len(), 3);
    opts.header = HeaderMode::Full;
    let last = build_notation(&custom, &opts, 0).header.pop().unwrap();
    assert!(matches!(last, HeaderLine::Tag { name, .. } if name == "FEN"));
}

#[test]
fn very_long_line_and_deep_variations_do_not_overflow() {
    // A long main line: knights shuffling.
    let mut g = Game::new();
    let mut at = g.root();
    for i in 0..5000 {
        let san = ["Nf3", "Nf6", "Ng1", "Ng8"][i % 4];
        at = mv(&mut g, at, san);
    }
    let doc = build_notation(&g, &NotationOptions::default(), 1);
    assert_eq!(
        doc.blocks
            .iter()
            .flat_map(|b| &b.inlines)
            .filter(|i| matches!(i, Inline::Move { .. }))
            .count(),
        5000
    );
    // Variations nested 1500 deep: each `Nf3` has a `Nc3` alternative.
    let mut g = Game::new();
    let mut at = g.root();
    for _ in 0..1500 {
        // `Nf3` stays the main line; the rest happens in the alternative,
        // which returns to the start position so the pattern can repeat.
        mv(&mut g, at, "Nf3");
        at = mv(&mut g, at, "Nc3");
        at = mv(&mut g, at, "Nf6");
        at = mv(&mut g, at, "Nb1");
        at = mv(&mut g, at, "Ng8");
    }
    for opts in [NotationOptions::default(), plain_opts()] {
        let doc = build_notation(&g, &opts, 1);
        assert!(!doc.blocks.is_empty());
        assert!(doc.blocks.iter().all(|b| b.indent <= MAX_INDENT));
    }
}

// ---- the oracle: the PGN writer ----

/// A game from a list of choices: `(node, move, decoration)`.
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
            game.add_nag(id, nag(u16::from(c % 17) + 1)).unwrap();
        }
        if c % 7 == 2 {
            game.add_nag(id, nag(u16::from(c % 5) + 10)).unwrap();
        }
        if c % 6 == 3 {
            game.set_comment_before(id, &format!("before {i}")).unwrap();
        }
        if i == 3 && c % 2 == 0 {
            game.set_initial_comment("start of game");
        }
    }
    game
}

fn writer_movetext(game: &Game) -> String {
    let opts = WriteOptions::default()
        .with_nags(NagOutput::Numeric)
        .with_move_number_space(false)
        .with_width(LineWidth::Unlimited);
    let pgn = game_to_string(game, &opts).unwrap();
    let (_, movetext) = pgn.split_once("\n\n").unwrap();
    normalize(&movetext.replace(['{', '}'], ""))
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// The flat text of the document is the movetext of the PGN writer
    /// (minus the braces around comments), in both layouts.
    #[test]
    fn flat_text_matches_the_pgn_writer(
        ops in prop::collection::vec((0usize..64, 0usize..64, any::<u8>()), 1..60),
        indented in any::<bool>(),
    ) {
        let game = random_game(&ops);
        let mut opts = plain_opts();
        if indented {
            opts.layout = NotationLayout::Indented;
        }
        let doc = build_notation(&game, &opts, 1);
        prop_assert_eq!(normalize(&doc.flat_text()), writer_movetext(&game));
    }
}

#[test]
fn the_generator_makes_variations_comments_and_nags() {
    let ops: Vec<(usize, usize, u8)> = (0..60)
        .map(|i| (i * 7 % 13, i * 5 % 11, u8::try_from(i * 13 % 251).unwrap()))
        .collect();
    let game = random_game(&ops);
    let text = writer_movetext(&game);
    assert!(text.contains('('), "no variation in {text}");
    assert!(text.contains("note"), "no comment in {text}");
    assert!(text.contains('$'), "no NAG in {text}");
    assert_eq!(
        normalize(&build_notation(&game, &plain_opts(), 1).flat_text()),
        text
    );
}

#[test]
fn oracle_on_the_handmade_game() {
    let (mut g, ids) = with_variation();
    g.set_comment(ids[0], "good").unwrap();
    g.set_comment_before(ids[3], "try this").unwrap();
    g.add_nag(ids[1], nag(14)).unwrap();
    g.set_initial_comment("hello");
    g.tags_mut().result = GameResult::White;
    let doc = build_notation(&g, &plain_opts(), 1);
    assert_eq!(normalize(&doc.flat_text()), writer_movetext(&g));
}

#[test]
fn default_options_are_the_scids_but_table_and_hidden_header() {
    // M2-5 D241/D249 replaced the Scid's layout and header defaults.
    let opts = NotationOptions::default();
    assert_eq!(opts.layout, NotationLayout::Table);
    assert!(opts.indent_comments && opts.symbolic_nags && !opts.move_number_space);
    assert_eq!(opts.header, HeaderMode::Hidden);
    assert_eq!(opts.hover_preview, HoverPreview::Delay(600));
    assert_eq!(HoverPreview::from_millis(0), HoverPreview::Off);
    assert_eq!(HoverPreview::from_millis(99_999).as_millis(), 5000);
}

// ---- performance (D168) ----

/// A deterministic pseudo-random number generator (xorshift), so the
/// benchmark game is the same on every machine.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 >> 33).unwrap_or(0)
    }
}

/// 300 plies on the main line and 150 variations of 1 to 3 moves, hanging
/// from random nodes (so they nest), with comments and NAGs on some moves.
pub(super) fn big_game() -> Game {
    let mut rng = Xorshift(0x9E37_79B9_7F4A_7C15);
    let mut game = Game::new();
    let mut nodes = vec![game.root()];
    let mut at = game.root();
    while nodes.len() <= 300 {
        let moves = game.position_at(at).unwrap().legal_moves();
        if moves.is_empty() {
            break;
        }
        at = game.add_move(at, moves[rng.next() % moves.len()]).unwrap();
        nodes.push(at);
    }
    let mut variations = 0;
    while variations < 150 {
        let parent = nodes[rng.next() % nodes.len()];
        let moves = game.position_at(parent).unwrap().legal_moves();
        if moves.is_empty() {
            continue;
        }
        let Ok((head, true)) = game.add_move_new(parent, moves[rng.next() % moves.len()]) else {
            continue;
        };
        variations += 1;
        nodes.push(head);
        let mut tail = head;
        for _ in 0..rng.next() % 3 {
            let next = game.position_at(tail).unwrap().legal_moves();
            if next.is_empty() {
                break;
            }
            tail = game.add_move(tail, next[rng.next() % next.len()]).unwrap();
            nodes.push(tail);
        }
    }
    for (i, &node) in nodes.iter().enumerate().skip(1) {
        if i % 9 == 0 {
            game.set_comment(node, "a short comment about the move")
                .unwrap();
        }
        if i % 5 == 0 {
            game.add_nag(node, nag(u16::try_from(i % 6).unwrap() + 1))
                .unwrap();
        }
    }
    game
}

/// `cargo test -p mychess-app --release -- --ignored notation_perf --nocapture`.
/// Goal: at most 5 ms for 300 moves with 150 variations.
#[test]
#[ignore = "timing: run with --release"]
fn notation_perf_300_moves_150_variations() {
    use std::time::Instant;
    let game = big_game();
    let max_depth = game
        .descendants(game.root())
        .unwrap()
        .map(|n| {
            game.ancestors(n)
                .unwrap()
                .filter(|&a| game.is_variation(a).unwrap_or(false))
                .count()
        })
        .max()
        .unwrap_or(0);
    let opts = NotationOptions::default();
    let mut times = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let doc = build_notation(&game, &opts, 1);
        times.push(start.elapsed());
        assert!(doc.blocks.len() > 150);
    }
    times.sort();
    let median = times[times.len() / 2];
    println!(
        "nodes={} max variation nesting={max_depth} median={median:?} min={:?} max={:?}",
        game.node_count(),
        times[0],
        times[times.len() - 1]
    );
    if !cfg!(debug_assertions) {
        assert!(median.as_millis() < 5, "too slow: {median:?}");
    }
}
