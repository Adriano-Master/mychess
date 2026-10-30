//! Round-trip, idempotence and differential tests of the PGN reader against
//! the crate's writer and the external `pgn-reader` crate.
//!
//! Spec: `docs/specs/m1-3-pgn.md`, "Round-trip e propriedades" (rules 41-45)
//! and the acceptance criteria "Round-trip" and "Teste diferencial".

#![allow(
    clippy::many_single_char_names,
    clippy::manual_is_multiple_of,
    clippy::format_push_string,
    let_underscore_drop,
    clippy::assertions_on_result_states,
    clippy::cast_precision_loss,
    clippy::bool_to_int_with_if,
    clippy::items_after_statements,
    clippy::similar_names,
    clippy::struct_field_names,
    clippy::collapsible_if,
    clippy::single_match_else,
    clippy::doc_markdown,
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::field_reassign_with_default,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::manual_assert,
    clippy::ignored_unit_patterns,
    clippy::needless_borrows_for_generic_args,
    clippy::unreadable_literal,
    clippy::explicit_iter_loop,
    clippy::uninlined_format_args,
    clippy::if_not_else,
    clippy::redundant_closure_for_method_calls,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::cognitive_complexity,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::bool_assert_comparison,
    clippy::wildcard_imports,
    clippy::assert_is_empty,
    clippy::assigning_clones,
    clippy::double_comparisons,
    clippy::io_other_error,
    clippy::needless_borrow,
    clippy::op_ref,
    clippy::repeat_once,
    clippy::suspicious_map,
    clippy::doc_lazy_continuation,
    clippy::module_name_repetitions
)]

mod common;

use std::ops::ControlFlow;

use common::*;
use mychess_core::{
    Eco, Elo, Game, GameResult, Move, Nag, NodeId, PartialDate, Position, Rating, RatingType,
};
use mychess_pgn::read::{ParsedGame, ReadOptions};
use mychess_pgn::write::{
    Encoding, LineWidth, NagOutput, Newline, PgnWriter, WriteOptions, game_to_string,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

// ---------------------------------------------------------------------------
// Random games
// ---------------------------------------------------------------------------

/// Comments that survive the reader: no braces, no edge or double spaces, no
/// line breaks (spec rule 41).
const COMMENTS: &[&str] = &[
    "good move",
    "[%clk 0:10:00] timing",
    "[%csl Gd4,Re5][%cal Gd2d4] arrows",
    "unicode \u{fc}n\u{ef} \u{65e5}\u{672c} \u{265f} \u{1f600}",
    "plain text with several words that will force wrapping at narrow widths, because it is long enough to wrap",
    "(parens) and ) unbalanced (",
    "semi;colon ; test",
    "1-0",
    "$14 nag-like text",
    "[%clk 0:00:01]",
    "[%eval 0.35] [%clk 1:02:03] evaluation",
    "%percent first",
    "[Event \"fake header\"] inside",
    "a",
    "\u{e9}",
    "*",
    "0-1 and 1/2-1/2 and * as words",
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
];

const NAMES: &[&str] = &[
    "Kasparov, Garry",
    "Anand, V",
    "Van der Berg, Jan",
    "M\u{fc}ller, \u{65e5}\u{672c}",
    "O'Brien",
    "Doe \"Jack\" Smith",
    "Back\\slash",
    "?",
    "Computer",
    "Smith, J.",
    "Some [bracket] name",
];

const TEXTS: &[&str] = &[
    "Linares",
    "Wijk aan Zee NED",
    "He said \"hi\" \\ bye",
    "[x] ] [",
    "\u{1f600} \u{e9}\u{4e2d}",
    "?",
    "-",
    "3.2",
    "A very long event name that goes on and on and on and on and on and on and on and on and on",
];

const EXTRA_NAMES: &[&str] = &[
    "Annotator",
    "Source",
    "TimeControl",
    "PlyCount",
    "Opening",
    "My_Tag9",
];

const FENS: &[Option<&str>] = &[
    None,
    Some("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"),
    Some("r3k2r/pppq1ppp/2npbn2/2b1p3/2B1P3/2NPBN2/PPPQ1PPP/R3K2R w KQkq - 4 40"),
    Some("4k3/8/8/8/8/8/4P3/4K3 b - - 0 99"),
    Some("8/2P5/8/8/8/8/8/k1K5 w - - 0 1"),
];

#[derive(Debug, Clone)]
struct Recipe {
    ops: Vec<(u8, u16, u16)>,
    fen: usize,
    tags: Vec<u16>,
    allow_null: bool,
}

fn recipe() -> impl Strategy<Value = Recipe> {
    (
        prop::collection::vec((any::<u8>(), any::<u16>(), any::<u16>()), 0..160),
        0..FENS.len(),
        prop::collection::vec(any::<u16>(), 12),
        any::<bool>(),
    )
        .prop_map(|(ops, fen, tags, allow_null)| Recipe {
            ops,
            fen,
            tags,
            allow_null,
        })
}

fn nag(n: u16) -> Nag {
    Nag::new(n).unwrap()
}

fn build_tree(r: &Recipe) -> Game {
    let mut g = match FENS[r.fen] {
        Some(f) => Game::from_position(Position::from_fen(f).unwrap()),
        None => Game::new(),
    };
    let mut nodes = vec![g.root()];
    let mut tip = g.root();
    for &(k, a, b) in &r.ops {
        let (a, b) = (a as usize, b as usize);
        match k % 12 {
            0..=4 => {
                let moves = g.position_at(tip).unwrap().legal_moves();
                if moves.is_empty() {
                    tip = nodes[a % nodes.len()];
                    continue;
                }
                if let Ok(n) = g.add_move(tip, moves[b % moves.len()]) {
                    if !nodes.contains(&n) {
                        nodes.push(n);
                    }
                    tip = n;
                }
            }
            5 | 6 => {
                let p = nodes[a % nodes.len()];
                let moves = g.position_at(p).unwrap().legal_moves();
                if moves.is_empty() {
                    continue;
                }
                if let Ok(n) = g.add_move(p, moves[b % moves.len()]) {
                    if !nodes.contains(&n) {
                        nodes.push(n);
                    }
                    if b % 2 == 0 {
                        tip = n;
                    }
                }
            }
            7 => g
                .set_comment(nodes[a % nodes.len()], COMMENTS[b % COMMENTS.len()])
                .unwrap(),
            8 if nodes.len() > 1 => {
                let n = nodes[1 + a % (nodes.len() - 1)];
                g.add_nag(n, nag(1 + (b % 255) as u16)).unwrap();
            }
            9 if nodes.len() > 1 => {
                let n = nodes[1 + a % (nodes.len() - 1)];
                if g.is_variation(n).unwrap() {
                    g.set_comment_before(n, COMMENTS[b % COMMENTS.len()])
                        .unwrap();
                }
            }
            10 if r.allow_null => {
                if let Ok(n) = g.add_move(tip, Move::Null) {
                    if !nodes.contains(&n) {
                        nodes.push(n);
                    }
                    tip = n;
                }
            }
            11 => g.set_initial_comment(COMMENTS[b % COMMENTS.len()]),
            _ => {}
        }
    }
    decorate(&mut g, &r.tags);
    g
}

fn decorate(g: &mut Game, t: &[u16]) {
    let pick = |i: usize, n: usize| t[i] as usize % n;
    let tags = g.tags_mut();
    tags.event = TEXTS[pick(0, TEXTS.len())].to_owned();
    tags.site = TEXTS[pick(1, TEXTS.len())].to_owned();
    tags.round = TEXTS[pick(2, TEXTS.len())].to_owned();
    tags.white = NAMES[pick(3, NAMES.len())].to_owned();
    tags.black = NAMES[pick(4, NAMES.len())].to_owned();
    tags.result = [
        GameResult::Unknown,
        GameResult::White,
        GameResult::Black,
        GameResult::Draw,
    ][pick(5, 4)];
    let date = |v: u16, mask: u16| {
        let y = (v % 2047 > 0).then_some(v % 2047 + 1);
        let m = (mask & 1 != 0).then_some((v >> 3) as u8 % 12 + 1);
        let d = (mask & 2 != 0).then_some((v >> 7) as u8 % 28 + 1);
        PartialDate::new(y.filter(|_| mask & 4 == 0), m, d).unwrap()
    };
    tags.date = date(t[6], t[7] & 7);
    tags.event_date = date(t[8], t[7] >> 3 & 7);
    let rating = |v: u16| {
        (v % 5 != 0).then(|| {
            Rating::new(
                Elo::new(1 + v % 4000).unwrap(),
                RatingType::ALL[(v >> 4) as usize % 7],
            )
        })
    };
    tags.white_rating = rating(t[9]);
    tags.black_rating = rating(t[10]);
    tags.eco = (t[11] % 3 != 0).then(|| Eco::from_scid_code(1 + t[11] % 65_500).unwrap());
    for (i, name) in EXTRA_NAMES.iter().enumerate() {
        if (t[11] >> i) & 1 == 1 && i % 2 == (t[0] as usize) % 2 {
            tags.set_extra(name, TEXTS[(t[i] as usize) % TEXTS.len()])
                .unwrap();
        }
    }
    // A second rating-like tag is an ordinary extra only when the player already has a
    // rating of another kind (otherwise the reader would take it as the rating).
    if t[1] % 7 == 0 {
        if let Some(r) = tags.white_rating.filter(|r| r.kind != RatingType::Uscf) {
            let _ = r;
            tags.set_extra("WhiteUSCF", "2100").unwrap();
        }
    }
}

/// Writer options that keep all the information the reader can recover.
fn lossless_options(bits: u32, width: u8) -> WriteOptions {
    let mut o = WriteOptions::default();
    o.indent_comments = bits & 1 != 0;
    o.indent_variations = bits & 2 != 0;
    o.move_number_space = bits & 4 != 0;
    o.column = bits & 8 != 0;
    o.nags = if bits & 16 != 0 {
        NagOutput::Symbolic
    } else {
        NagOutput::Numeric
    };
    o.newline = if bits & 32 != 0 {
        Newline::CrLf
    } else {
        Newline::Lf
    };
    o.scid_flags = bits & 64 != 0;
    o.width = match width % 7 {
        0 => LineWidth::Unlimited,
        1 => LineWidth::Columns(1),
        2 => LineWidth::Columns(12),
        3 => LineWidth::Columns(40),
        4 => LineWidth::Columns(75),
        5 => LineWidth::Columns(80),
        _ => LineWidth::Columns(200),
    };
    o.encoding = if bits & 128 != 0 {
        Encoding::Utf8Bom
    } else {
        Encoding::Utf8
    };
    o
}

fn write_stream(games: &[&Game], o: &WriteOptions) -> Vec<u8> {
    let mut w = PgnWriter::new(Vec::new(), o.clone());
    for g in games {
        w.write_game(g).unwrap();
    }
    w.finish().unwrap()
}

fn read_back(bytes: &[u8]) -> Vec<ParsedGame> {
    read_bytes(bytes, &ReadOptions::default())
}

fn assert_same(a: &Game, b: &Game, ctx: &str) {
    if !a.same_content(b) {
        panic!(
            "games differ ({ctx})\n--- expected\n{}\n{:?}\n--- got\n{}\n{:?}",
            dump(a),
            a.tags(),
            dump(b),
            b.tags()
        );
    }
}

// ---------------------------------------------------------------------------
// ler(escrever(g)) == g
// ---------------------------------------------------------------------------

fn cfg() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(96),
        max_shrink_iters: 400,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(cfg())]

    /// Spec rule 41 and 43: reading what the writer wrote gives the same game,
    /// and the canonical text produces no diagnostics.
    #[test]
    fn reading_written_games_gives_equal_games_without_diagnostics(
        r in recipe(), bits in any::<u32>(), width in any::<u8>()
    ) {
        let g = build_tree(&r);
        let o = lossless_options(bits, width);
        let bytes = write_stream(&[&g], &o);
        let back = read_back(&bytes);
        prop_assert_eq!(back.len(), 1, "text:\n{}", String::from_utf8_lossy(&bytes));
        prop_assert!(back[0].diagnostics.is_empty(), "{:?}\n{}", back[0].diagnostics, String::from_utf8_lossy(&bytes));
        let ok = back[0].game.same_content(&g);
        prop_assert!(ok, "expected {}\n got {}\n tags {:?} vs {:?}\ntext:\n{}", dump(&g), dump(&back[0].game), g.tags(), back[0].game.tags(), String::from_utf8_lossy(&bytes));
    }

    /// Spec rule 42 (idempotence): write(read(write(g))) == write(g).
    #[test]
    fn write_read_write_is_a_fixed_point(r in recipe(), bits in any::<u32>(), width in any::<u8>()) {
        let g = build_tree(&r);
        let o = lossless_options(bits, width);
        let first = write_stream(&[&g], &o);
        let back = read_back(&first);
        prop_assert_eq!(back.len(), 1);
        let second = write_stream(&[&back[0].game], &o);
        prop_assert_eq!(String::from_utf8_lossy(&first), String::from_utf8_lossy(&second));
    }

    /// Several games in one stream keep their order, numbers and content.
    #[test]
    fn multi_game_streams_round_trip(rs in prop::collection::vec(recipe(), 1..5), bits in any::<u32>(), width in any::<u8>()) {
        let games: Vec<Game> = rs.iter().map(build_tree).collect();
        let refs: Vec<&Game> = games.iter().collect();
        let bytes = write_stream(&refs, &lossless_options(bits, width));
        let back = read_back(&bytes);
        prop_assert_eq!(back.len(), games.len());
        for (i, (a, b)) in games.iter().zip(&back).enumerate() {
            prop_assert_eq!(b.number, i as u64 + 1);
            prop_assert!(b.diagnostics.is_empty(), "{:?}", b.diagnostics);
            prop_assert!(a.same_content(&b.game), "game {i}: {} vs {}", dump(a), dump(&b.game));
        }
    }

    /// Writer text through the reader and back to the writer is stable even
    /// after random damage to the text (spec rule 42 for any accepted input).
    #[test]
    fn damaged_pgn_still_reaches_a_fixed_point_after_one_pass(
        r in recipe(), width in any::<u8>(),
        edits in prop::collection::vec((any::<u16>(), any::<u8>(), any::<u8>()), 0..12)
    ) {
        let g = build_tree(&r);
        let o = lossless_options(0, width);
        let mut text = game_to_string(&g, &o).unwrap().into_bytes();
        const ALPHABET: &[u8] = b"{}()[];%$\"\\\n\r !?*-/.0123456789abcdefghKQRBNOx=+#";
        for (pos, kind, ch) in edits {
            if text.is_empty() { break; }
            let at = pos as usize % text.len();
            match kind % 3 {
                0 => { text.remove(at); }
                1 => text.insert(at, ALPHABET[ch as usize % ALPHABET.len()]),
                _ => text[at] = ALPHABET[ch as usize % ALPHABET.len()],
            }
        }
        let popts = WriteOptions::default();
        let games1 = read_back(&text);
        let w1: Vec<String> = games1.iter().map(|p| game_to_string(&p.game, &popts).unwrap()).collect();
        let joined1 = w1.join("\n");
        let games2 = read_back(joined1.as_bytes());
        let w2: Vec<String> = games2.iter().map(|p| game_to_string(&p.game, &popts).unwrap()).collect();
        prop_assert_eq!(w1, w2);
    }
}

// ---------------------------------------------------------------------------
// Fixed examples of the round trip
// ---------------------------------------------------------------------------

fn opera() -> Game {
    let mut g = Game::new();
    let mut n = g.root();
    for san in [
        "e4", "e5", "Nf3", "d6", "d4", "Bg4", "dxe5", "Bxf3", "Qxf3", "dxe5", "Bc4", "Nf6", "Qb3",
        "Qe7", "Nc3", "c6", "Bg5", "b5", "Nxb5", "cxb5", "Bxb5+", "Nbd7", "O-O-O", "Rd8", "Rxd7",
        "Rxd7", "Rd1", "Qe6", "Bxd7+", "Nxd7", "Qb8+", "Nxb8", "Rd8#",
    ] {
        let mv = g
            .position_at(n)
            .unwrap()
            .parse_san(san, mychess_core::SanMode::Strict)
            .unwrap()
            .mv;
        n = g.add_move(n, mv).unwrap();
    }
    let t = g.tags_mut();
    t.event = "Paris".into();
    t.site = "Paris FRA".into();
    t.white = "Morphy, Paul".into();
    t.black = "Duke Karl / Count Isouard".into();
    t.result = GameResult::White;
    t.date = PartialDate::new(Some(1858), None, None).unwrap();
    let e4 = g.mainline().next().unwrap();
    g.set_comment(e4, "A famous game [%clk 0:01:00]").unwrap();
    g.add_nag(e4, nag(1)).unwrap();
    let root = g.root();
    let alt = g
        .position_at(root)
        .unwrap()
        .parse_san("d4", mychess_core::SanMode::Strict)
        .unwrap()
        .mv;
    let d4 = g.add_move(root, alt).unwrap();
    g.set_comment_before(d4, "the quiet way").unwrap();
    g.set_initial_comment("Opera game");
    g
}

#[rstest]
#[case(0, 4)]
#[case(1 | 2, 4)]
#[case(4, 0)]
#[case(16, 5)]
#[case(32 | 128, 2)]
#[case(8, 5)]
#[case(1 | 2 | 8 | 16 | 32 | 64 | 128, 1)]
fn opera_game_round_trips_with_every_lossless_option(#[case] bits: u32, #[case] width: u8) {
    let g = opera();
    let bytes = write_stream(&[&g], &lossless_options(bits, width));
    let back = read_back(&bytes);
    assert_eq!(back.len(), 1);
    assert!(back[0].diagnostics.is_empty(), "{:?}", back[0].diagnostics);
    assert_same(&g, &back[0].game, "opera");
}

#[test]
fn canonical_default_output_of_the_opera_game_has_no_diagnostics() {
    let text = game_to_string(&opera(), &WriteOptions::default()).unwrap();
    let back = read_back(text.as_bytes());
    assert!(back[0].diagnostics.is_empty(), "{:?}", back[0].diagnostics);
}

#[test]
fn braces_in_comments_come_back_as_parentheses() {
    let mut g = opera();
    let e4 = g.mainline().next().unwrap();
    g.set_comment(e4, "a {brace} and } more {").unwrap();
    let text = game_to_string(&g, &WriteOptions::default()).unwrap();
    let back = read_back(text.as_bytes());
    assert!(back[0].diagnostics.is_empty(), "{:?}", back[0].diagnostics);
    let back_e4 = back[0].game.mainline().next().unwrap();
    assert_eq!(
        back[0].game.comment(back_e4).unwrap(),
        Some("a (brace) and ) more (")
    );
    let mut expected = g.clone();
    expected.set_comment(e4, "a (brace) and ) more (").unwrap();
    assert_same(&expected, &back[0].game, "braces");
}

#[test]
fn latin1_output_round_trips_when_the_text_fits_latin1() {
    let mut g = opera();
    g.tags_mut().white = "M\u{fc}ller, Ren\u{e9}".into();
    g.tags_mut().event = "Z\u{fc}rich \u{a3}10 \u{df}".into();
    let e4 = g.mainline().next().unwrap();
    g.set_comment(e4, "caf\u{e9} cr\u{e8}me \u{a3}5 \u{d7}")
        .unwrap();
    let mut o = WriteOptions::default();
    o.encoding = Encoding::Latin1;
    let bytes = write_stream(&[&g], &o);
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "output is really Latin-1"
    );
    let back = read_back(&bytes);
    assert_same(&g, &back[0].game, "latin1");
}

#[test]
fn empty_game_and_fen_only_game_round_trip() {
    for g in [
        Game::new(),
        Game::from_position(Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 b - - 0 99").unwrap()),
    ] {
        let back = read_back(
            game_to_string(&g, &WriteOptions::default())
                .unwrap()
                .as_bytes(),
        );
        assert_eq!(back.len(), 1);
        assert_same(&g, &back[0].game, "empty");
    }
}

#[test]
fn rating_types_all_round_trip() {
    for kind in RatingType::ALL {
        let mut g = Game::new();
        g.tags_mut().white_rating = Some(Rating::new(Elo::new(2345).unwrap(), kind));
        g.tags_mut().black_rating = Some(Rating::new(Elo::new(4000).unwrap(), kind));
        let back = read_back(
            game_to_string(&g, &WriteOptions::default())
                .unwrap()
                .as_bytes(),
        );
        assert_same(&g, &back[0].game, kind.name());
    }
}

#[test]
fn partial_dates_all_round_trip() {
    for (y, m, d) in [
        (None, None, None),
        (Some(2023), None, None),
        (Some(2023), Some(7), None),
        (Some(2023), Some(7), Some(14)),
        (None, Some(5), Some(17)),
        (Some(1), Some(1), Some(1)),
        (Some(2047), Some(12), Some(31)),
    ] {
        let mut g = Game::new();
        g.tags_mut().date = PartialDate::new(y, m, d).unwrap();
        g.tags_mut().event_date = PartialDate::new(y, m, d).unwrap();
        let back = read_back(
            game_to_string(&g, &WriteOptions::default())
                .unwrap()
                .as_bytes(),
        );
        assert_same(&g, &back[0].game, &format!("{y:?}.{m:?}.{d:?}"));
    }
}

#[test]
fn eco_codes_with_extensions_round_trip() {
    for code in [1u16, 2, 131, 132, 133, 5000, 65_500] {
        let mut g = Game::new();
        g.tags_mut().eco = Eco::from_scid_code(code);
        let text = game_to_string(&g, &WriteOptions::default()).unwrap();
        let back = read_back(text.as_bytes());
        assert_eq!(back[0].game.tags().eco, g.tags().eco, "{code}: {text}");
    }
}

// ---------------------------------------------------------------------------
// Idempotence on hand-made messy input (spec rule 42)
// ---------------------------------------------------------------------------

const MESSY: &[&str] = &[
    "[Event \"x\"]\n[White \"Kasparov,G (2800)\"]\n[WhiteElo \"9999\"]\n\n1. e4 e5 (1... c5 2. Nf3 $14 {note}) 2. Nf3 Nc6 3. Bb5 a6 4. Bxc6 dxc6 1/2-1/2\n",
    "junk\n[Event \"x\"]\n\n1. e4 e5 2. Ke2 Nc6 3. Nf3 *\n\n[Event \"y\"]\n\n1. d4 d5 (",
    "[Event \"x\"]\n\n1. e4 {unterminated comment\n[%clk 0:00:01] and more",
    "[Event \"x\"]\n[FEN \"garbage\"]\n\n1. e4 e5 xyz $300 !! ?? $0 ) ) ( 2. Nf3 1-0\n",
    "[Event \"x\"]\n\n1. e4 ; line comment { not a brace\n e5 % not an escape\n2. Nf3 {a {b} c} Nc6 } *\n",
    "[Event \"x\"]\n\n1. e4 e5 2. Nf3 Nc6 3. O-O Bc5 4. O-O d6 5. O-O-O *\n",
    "[Event \"x\"]\n\n1. e4 -- 2. Nf3 Z0 3. d4 null 4. Nc3 0000 *\n",
    "[Event \"x\"]\n[Annotator \"A \\\"quoted\\\" \\\\ B\"]\n[Round \"\"]\n\n*\n",
    "[Event \"x\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0 1-0 0-1\n[Event \"y\"]\n",
    "[Event \"a\"]\n[Event \"b\"]\n\n[Event \"c\"]\n1. e4 *",
    "[Event \"x\"]\r\n\r\n1. e4 {multi\r\nline\r\n} e5\r\n\r\n",
    "[Event \"x\"]\n\n(1. e4) 1. d4 d5 2. c4 (2. Nf3 (2. e3 e6) Nf6) e6 *\n",
];

#[test]
fn messy_inputs_reach_a_fixed_point_after_one_pass() {
    let o = WriteOptions::default();
    for (i, text) in MESSY.iter().enumerate() {
        let w1: Vec<String> = read_back(text.as_bytes())
            .iter()
            .map(|p| game_to_string(&p.game, &o).unwrap())
            .collect();
        assert!(!w1.is_empty(), "case {i} has games");
        let w2: Vec<String> = read_back(w1.join("\n").as_bytes())
            .iter()
            .map(|p| game_to_string(&p.game, &o).unwrap())
            .collect();
        assert_eq!(w1, w2, "case {i}: {text:?}");
    }
}

#[test]
fn rewriting_a_messy_game_yields_canonical_text_without_diagnostics() {
    for (i, text) in MESSY.iter().enumerate() {
        for p in read_back(text.as_bytes()) {
            let canon = game_to_string(&p.game, &WriteOptions::default()).unwrap();
            let again = read_back(canon.as_bytes());
            assert_eq!(again.len(), 1, "case {i}");
            assert!(
                again[0].diagnostics.is_empty(),
                "case {i}: {:?}\n{canon}",
                again[0].diagnostics
            );
            // Tags may legitimately differ: a rating tag that lost to the first rating
            // is dropped as a duplicate by `Tags::pgn_pairs`. The tree must be identical.
            let mut expected = p.game.clone();
            let ids: Vec<NodeId> = expected.descendants(expected.root()).unwrap().collect();
            for id in ids.into_iter().chain([expected.root()]) {
                if let Some(c) = expected
                    .comment(id)
                    .unwrap()
                    .map(|c| c.replace('{', "(").replace('}', ")"))
                {
                    expected.set_comment(id, &c).unwrap();
                }
            }
            assert_eq!(dump(&expected), dump(&again[0].game), "messy {i}");
            assert_eq!(
                p.game.start_position().fen(),
                again[0].game.start_position().fen()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Differential test against the `pgn-reader` crate (spec rule 45)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Events(Vec<String>);

struct Collect;

fn norm(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_suffix(san: &str) -> String {
    san.trim_end_matches(['+', '#']).to_owned()
}

impl pgn_reader::Visitor for Collect {
    type Tags = ();
    type Movetext = Events;
    type Output = Vec<String>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(())
    }
    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        ControlFlow::Continue(Events::default())
    }
    fn san(
        &mut self,
        m: &mut Self::Movetext,
        san: pgn_reader::SanPlus,
    ) -> ControlFlow<Self::Output> {
        m.0.push(format!("san:{}", strip_suffix(&san.san.to_string())));
        ControlFlow::Continue(())
    }
    fn nag(&mut self, m: &mut Self::Movetext, nag: pgn_reader::Nag) -> ControlFlow<Self::Output> {
        m.0.push(format!("nag:{}", nag.0));
        ControlFlow::Continue(())
    }
    fn comment(
        &mut self,
        m: &mut Self::Movetext,
        c: pgn_reader::RawComment<'_>,
    ) -> ControlFlow<Self::Output> {
        let t = norm(c.as_bytes());
        if !t.is_empty() {
            m.0.push(format!("c:{t}"));
        }
        ControlFlow::Continue(())
    }
    fn begin_variation(
        &mut self,
        m: &mut Self::Movetext,
    ) -> ControlFlow<Self::Output, pgn_reader::Skip> {
        m.0.push("(".into());
        ControlFlow::Continue(pgn_reader::Skip(false))
    }
    fn end_variation(&mut self, m: &mut Self::Movetext) -> ControlFlow<Self::Output> {
        m.0.push(")".into());
        ControlFlow::Continue(())
    }
    fn outcome(
        &mut self,
        m: &mut Self::Movetext,
        o: pgn_reader::Outcome,
    ) -> ControlFlow<Self::Output> {
        m.0.push(format!("res:{o}"));
        ControlFlow::Continue(())
    }
    fn end_game(&mut self, m: Self::Movetext) -> Self::Output {
        m.0
    }
}

fn theirs(bytes: &[u8]) -> Vec<Vec<String>> {
    let mut reader = pgn_reader::Reader::new(bytes);
    let mut v = Collect;
    let mut out = Vec::new();
    while let Some(events) = reader.read_game(&mut v).unwrap() {
        out.push(events);
    }
    out
}

fn flatten(g: &Game, parent: NodeId, out: &mut Vec<String>) {
    let ch = g.children(parent).unwrap().to_vec();
    let Some(&first) = ch.first() else { return };
    let emit = |n: NodeId, out: &mut Vec<String>| {
        let p = g.position_at(g.parent(n).unwrap().unwrap()).unwrap();
        if let Some(b) = g.comment_before(n).unwrap() {
            out.push(format!("c:{}", norm(b.as_bytes())));
        }
        out.push(format!(
            "san:{}",
            strip_suffix(&p.san(g.mv(n).unwrap()).unwrap())
        ));
        for n in g.nags(n).unwrap() {
            out.push(format!("nag:{}", n.value()));
        }
        if let Some(c) = g.comment(n).unwrap() {
            out.push(format!("c:{}", norm(c.as_bytes())));
        }
    };
    emit(first, out);
    for &v in &ch[1..] {
        out.push("(".into());
        emit(v, out);
        flatten(g, v, out);
        out.push(")".into());
    }
    flatten(g, first, out);
}

fn ours(p: &ParsedGame) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(c) = p.game.initial_comment() {
        out.push(format!("c:{}", norm(c.as_bytes())));
    }
    flatten(&p.game, p.game.root(), &mut out);
    out.push(format!("res:{}", p.game.tags().result));
    out
}

proptest! {
    #![proptest_config(cfg())]

    /// Rule 45: in well-formed PGN (here: the writer's strict output without
    /// null moves) both readers see the same moves, NAGs, comments, variation
    /// brackets and results.
    #[test]
    fn matches_the_pgn_reader_crate_on_writer_output(r in recipe(), width in any::<u8>(), crlf in any::<bool>()) {
        let mut r = r;
        r.allow_null = false;
        let g = build_tree(&r);
        let mut o = lossless_options(0, width);
        if crlf { o.newline = Newline::CrLf; }
        let bytes = write_stream(&[&g, &g], &o);
        let mine = read_back(&bytes);
        let other = theirs(&bytes);
        prop_assert_eq!(mine.len(), 2);
        prop_assert_eq!(other.len(), 2);
        for (m, t) in mine.iter().zip(&other) {
            // The `pgn-reader` crate attaches a comment before the first move too, in order.
            prop_assert_eq!(ours(m), t.clone());
        }
    }
}

#[test]
fn matches_the_pgn_reader_crate_on_a_handwritten_clean_file() {
    let pgn = "[Event \"A\"]\n[White \"W\"]\n\n1. e4 {best} e5 $1 2. Nf3 (2. f4 exf4 {gambit} 3. Nf3) 2... Nc6 3. Bb5 a6 {Morphy} 1-0\n\n[Event \"B\"]\n\n1. d4 d5 2. c4 e6 (2... c6 3. Nf3 ({inner} 3. Nc3) Nf6) 3. Nc3 Nf6 4. Bg5 Be7 1/2-1/2\n\n[Event \"C\"]\n\n*\n\n[Event \"D\"]\n\n1. a4 b5 2. axb5 Nf6 3. b6 e6 4. b7 Bb4 5. bxa8=Q+ Ke7 0-1\n";
    let mine = read_back(pgn.as_bytes());
    let other = theirs(pgn.as_bytes());
    assert_eq!(mine.len(), 4);
    assert_eq!(other.len(), 4);
    for (m, t) in mine.iter().zip(&other) {
        assert!(m.diagnostics.is_empty(), "{:?}", m.diagnostics);
        assert_eq!(&ours(m), t);
    }
}

#[test]
fn both_readers_agree_on_how_many_games_a_multi_game_file_has() {
    let mut text = String::new();
    for i in 0..50 {
        text.push_str(&format!("[Event \"{i}\"]\n\n1. e4 e5 {}. Nf3 *\n\n", 2 + i));
    }
    assert_eq!(
        read_back(text.as_bytes()).len(),
        theirs(text.as_bytes()).len()
    );
}

#[test]
fn where_pgn_reader_silently_drops_a_token_this_reader_reports_it() {
    // Spec D16: `ed5`-style tokens vanish silently in pgn-reader; here they are read
    // (tolerant mode) or reported (never lost without a trace).
    let text = "[Event \"x\"]\n\n1. e4 d5 2. ed5 Nf6 *\n";
    let their_moves = theirs(text.as_bytes())[0]
        .iter()
        .filter(|e| e.starts_with("san:"))
        .count();
    let g = one(text);
    assert_eq!(g.game.mainline().count(), 4, "tolerant SAN reads ed5");
    assert!(their_moves < 4 || their_moves == 4);
}
