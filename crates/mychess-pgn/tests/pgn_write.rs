//! Black-box tests of the PGN writer (`mychess_pgn::write`).
//!
//! The cases come from `docs/specs/m1-3-pgn.md` (decisions D31-D37, "Regras de
//! escrita" and the writer acceptance criteria) and from Scid's
//! `Game::WritePGN` / `Game::WriteMoveList`, not from the implementation. Only
//! the public API is used, and nothing here depends on `mychess_pgn::read`:
//! the property tests verify the output with a small movetext walker of their
//! own.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::many_single_char_names,
    clippy::missing_panics_doc,
    clippy::similar_names,
    clippy::type_complexity
)]

use std::io::{self, Write};
use std::time::Instant;

use mychess_core::{
    Color, Eco, Elo, Game, GameResult, Move, Nag, NodeId, PartialDate, Position, Rating,
    RatingType, SanMode,
};
use mychess_pgn::write::{
    BraceHandling, Encoding, Header, LineWidth, NagOutput, Newline, NullMoves, PgnWriter, TagSet,
    WriteOptions, game_to_string, write_game,
};
use proptest::prelude::*;
use rstest::rstest;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn nag(n: u16) -> Nag {
    Nag::new(n).unwrap()
}

fn play(g: &mut Game, parent: NodeId, san: &str) -> NodeId {
    let mv = g
        .position_at(parent)
        .unwrap()
        .parse_san(san, SanMode::Strict)
        .unwrap_or_else(|e| panic!("test setup: {san} is not legal: {e:?}"))
        .mv;
    g.add_move(parent, mv).unwrap()
}

/// Plays a space separated list of SAN moves from `from`; returns the last node.
fn line(g: &mut Game, from: NodeId, sans: &str) -> NodeId {
    sans.split_whitespace()
        .fold(from, |cur, san| play(g, cur, san))
}

fn with(f: impl FnOnce(&mut WriteOptions)) -> WriteOptions {
    let mut o = WriteOptions::default();
    f(&mut o);
    o
}

fn render(g: &Game, o: &WriteOptions) -> String {
    game_to_string(g, o).unwrap()
}

/// The text after the header (everything after the first blank line).
fn movetext(g: &Game, o: &WriteOptions) -> String {
    let text = render(g, o);
    let sep = match o.newline {
        Newline::CrLf => "\r\n\r\n",
        _ => "\n\n",
    };
    text.split_once(sep)
        .unwrap_or_else(|| panic!("no blank line in {text:?}"))
        .1
        .to_owned()
}

fn mt_default(g: &Game) -> String {
    movetext(g, &WriteOptions::default())
}

fn bytes_of(g: &Game, o: &WriteOptions) -> Vec<u8> {
    let mut out = Vec::new();
    write_game(&mut out, g, o).unwrap();
    out
}

fn latin1_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// `1.e4 e5 (1...c5 2.Nf3) 2.Nf3` as a tree.
fn sample_tree() -> Game {
    let mut g = Game::new();
    let root = g.root();
    let e4 = play(&mut g, root, "e4");
    let e5 = play(&mut g, e4, "e5");
    line(&mut g, e4, "c5 Nf3");
    play(&mut g, e5, "Nf3");
    g
}

const BLACK_TO_MOVE_FEN: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";

fn fen_game(fen: &str) -> Game {
    Game::from_position(Position::from_fen(fen).unwrap())
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

const COMMENT_POOL: &[&str] = &[
    "good move",
    "a {brace} here",
    "close } only",
    "open { only",
    "[%clk 0:10:00] timing",
    "[%csl Gd4,Re5][%cal Gd2d4] arrows",
    "unicode \u{fc}n\u{ef} \u{65e5}\u{672c} \u{265f} \u{1f600}",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "plain text with several words that will force wrapping at narrow widths, because it is long enough",
    "(parens) and ) unbalanced (",
    "semi;colon ; test",
    "1-0",
    "$14 nag-like text",
    "[%clk 0:00:01]",
    "{}",
    "[%eval 0.35] [%clk 1:02:03] evaluation",
];

/// Builds a random but legal game tree from a list of operations.
fn build(ops: &[(u8, u16, u16)], fen: Option<&str>) -> Game {
    let mut g = match fen {
        Some(f) => fen_game(f),
        None => Game::new(),
    };
    let mut nodes = vec![g.root()];
    let mut tip = g.root();
    for &(k, a, b) in ops {
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
            7 => {
                let n = nodes[a % nodes.len()];
                g.set_comment(n, COMMENT_POOL[b % COMMENT_POOL.len()])
                    .unwrap();
            }
            8 if nodes.len() > 1 => {
                let n = nodes[1 + a % (nodes.len() - 1)];
                g.add_nag(n, nag(1 + (b % 255) as u16)).unwrap();
            }
            9 if nodes.len() > 1 => {
                let n = nodes[1 + a % (nodes.len() - 1)];
                g.set_comment_before(n, COMMENT_POOL[b % COMMENT_POOL.len()])
                    .unwrap();
            }
            10 => {
                if let Ok(n) = g.add_move(tip, Move::Null) {
                    if !nodes.contains(&n) {
                        nodes.push(n);
                    }
                    tip = n;
                }
            }
            11 => g.set_initial_comment(COMMENT_POOL[b % COMMENT_POOL.len()]),
            _ => {}
        }
    }
    g
}

fn random_ops(seed: u64, n: usize) -> Vec<(u8, u16, u16)> {
    let mut rng = Lcg(seed);
    (0..n)
        .map(|_| (rng.next() as u8, rng.next() as u16, rng.next() as u16))
        .collect()
}

/// A long, deterministic game with variations and comments of all shapes.
fn busy_game(seed: u64) -> Game {
    build(&random_ops(seed, 500), None)
}

// ---------------------------------------------------------------------------
// Output scanners (independent of the crate's reader)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Open,
    Close,
    Comment(String),
    Word(String),
}

fn tokenize(mt: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = mt.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            out.push(Tok::Open);
            i += 1;
        } else if c == ')' {
            out.push(Tok::Close);
            i += 1;
        } else if c == '{' {
            let mut j = i + 1;
            let mut s = String::new();
            loop {
                match cs.get(j) {
                    None => return Err("unterminated comment".into()),
                    Some('}') => break,
                    Some('{') => return Err("open brace inside a comment".into()),
                    Some(&ch) => s.push(ch),
                }
                j += 1;
            }
            out.push(Tok::Comment(s));
            i = j + 1;
        } else if c == '}' {
            return Err("stray close brace".into());
        } else {
            let mut j = i;
            let mut s = String::new();
            while j < cs.len() && !cs[j].is_whitespace() && !matches!(cs[j], '(' | ')' | '{' | '}')
            {
                s.push(cs[j]);
                j += 1;
            }
            out.push(Tok::Word(s));
            i = j;
        }
    }
    Ok(out)
}

/// Maximum parenthesis nesting outside comments (and checks balance).
fn paren_depth(mt: &str) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut max = 0usize;
    let mut in_comment = false;
    for c in mt.chars() {
        match (in_comment, c) {
            (false, '{') => in_comment = true,
            (true, '{') => return Err("open brace inside a comment".into()),
            (true, '}') => in_comment = false,
            (false, '}') => return Err("stray close brace".into()),
            (false, '(') => {
                depth += 1;
                max = max.max(depth);
            }
            (false, ')') => depth = depth.checked_sub(1).ok_or("unbalanced )")?,
            _ => {}
        }
    }
    if in_comment {
        return Err("unterminated comment".into());
    }
    if depth != 0 {
        return Err("unclosed (".into());
    }
    Ok(max)
}

fn is_result(w: &str) -> bool {
    matches!(w, "1-0" | "0-1" | "1/2-1/2" | "*")
}

struct Cursor {
    tip: NodeId,
    parent: Option<NodeId>,
    need_number: bool,
}

/// Replays the movetext into a fresh `Game`, verifying SAN legality, move
/// numbers and nesting on the way. Does not use the crate's reader.
fn walk(src: &Game, o: &WriteOptions, toks: &[Tok]) -> Result<Game, String> {
    let mut g = Game::from_position(src.start_position().clone());
    let mut stack: Vec<Cursor> = Vec::new();
    let mut cur = Cursor {
        tip: g.root(),
        parent: None,
        need_number: true,
    };
    let mut pending: Option<(u32, usize)> = None;
    let mut last_was_move = false;
    let mut saw_result = false;
    let symbolic = o.nags == NagOutput::Symbolic;
    for (idx, t) in toks.iter().enumerate() {
        if saw_result {
            return Err("tokens after the result".into());
        }
        match t {
            Tok::Open => {
                if pending.is_some() {
                    return Err("move number before (".into());
                }
                let p = cur.parent.ok_or("variation without a move to replace")?;
                let outer = std::mem::replace(
                    &mut cur,
                    Cursor {
                        tip: p,
                        parent: None,
                        need_number: true,
                    },
                );
                stack.push(outer);
                last_was_move = false;
            }
            Tok::Close => {
                if pending.is_some() {
                    return Err("move number before )".into());
                }
                cur = stack.pop().ok_or("unbalanced )")?;
                cur.need_number = true;
                last_was_move = false;
            }
            Tok::Comment(_) => {
                if pending.is_some() {
                    return Err("move number before a comment".into());
                }
                cur.need_number = true;
                last_was_move = false;
            }
            Tok::Word(w) => {
                if is_result(w) {
                    if idx != toks.len() - 1 {
                        return Err(format!("result {w} not at the end"));
                    }
                    if w != src.tags().result.as_str() {
                        return Err(format!("result {w} differs from the Result tag"));
                    }
                    saw_result = true;
                    continue;
                }
                if w.contains(';') || w.starts_with('%') {
                    return Err(format!("dangerous word {w:?}"));
                }
                if let Some(num) = w.strip_prefix('$') {
                    let v: u16 = num.parse().map_err(|_| format!("bad NAG {w}"))?;
                    Nag::new(v).map_err(|_| format!("bad NAG {w}"))?;
                    if !last_was_move {
                        return Err(format!("NAG {w} not after a move"));
                    }
                    continue;
                }
                let digits = w.chars().take_while(char::is_ascii_digit).count();
                let mut rest = w.as_str();
                if digits > 0 {
                    let after = &w[digits..];
                    let dots = after.chars().take_while(|c| *c == '.').count();
                    if dots > 0 {
                        let n: u32 = w[..digits].parse().map_err(|_| "bad number".to_owned())?;
                        pending = Some((n, dots));
                        rest = &after[dots..];
                        if rest.is_empty() {
                            if !o.move_number_space {
                                return Err(format!("number {w} should be glued to the move"));
                            }
                            continue;
                        } else if o.move_number_space {
                            return Err(format!("number glued to move in {w}"));
                        }
                    }
                }
                // Symbolic NAGs: glued `!`/`?` or a standalone symbol.
                let core = if symbolic {
                    rest.trim_end_matches(['!', '?'])
                } else {
                    rest
                };
                if symbolic
                    && core != "--"
                    && pending.is_none()
                    && (core.is_empty() || Nag::parse(rest).is_some())
                {
                    let pos = g.position_at(cur.tip).unwrap();
                    let looks_like_move =
                        !core.starts_with("--") && pos.parse_san(core, SanMode::Strict).is_ok();
                    if core.is_empty() || !looks_like_move {
                        if Nag::parse(rest).is_none() {
                            return Err(format!("unknown symbol {rest:?}"));
                        }
                        if !last_was_move {
                            return Err(format!("symbolic NAG {rest} not after a move"));
                        }
                        continue;
                    }
                }
                let pos = g.position_at(cur.tip).unwrap().clone();
                let is_null = core == "--";
                let mv = if is_null {
                    if o.null_moves != NullMoves::Keep {
                        return Err("null move survived ConvertToComment".into());
                    }
                    Move::Null
                } else {
                    let parsed = pos
                        .parse_san(core, SanMode::Strict)
                        .map_err(|e| format!("illegal or unreadable move {core:?}: {e:?}"))?;
                    if !parsed.warnings.is_empty() {
                        return Err(format!("warnings on {core:?}"));
                    }
                    let canon = pos.san(parsed.mv).map_err(|e| format!("{e:?}"))?;
                    if canon != core {
                        return Err(format!("{core:?} is not the canonical SAN {canon:?}"));
                    }
                    parsed.mv
                };
                let white = pos.turn() == Color::White;
                let required = white || cur.need_number;
                match pending {
                    Some((n, d)) => {
                        if n != pos.fullmove_number() {
                            return Err(format!(
                                "move number {n} but position is at {}",
                                pos.fullmove_number()
                            ));
                        }
                        if d != if white { 1 } else { 3 } {
                            return Err(format!("wrong number of dots ({d}) for {core}"));
                        }
                        if !required {
                            return Err(format!("unneeded move number before {core}"));
                        }
                    }
                    None if required => return Err(format!("missing move number before {core}")),
                    None => {}
                }
                pending = None;
                let id = g.add_move(cur.tip, mv).map_err(|e| format!("{e:?}"))?;
                cur.parent = Some(cur.tip);
                cur.tip = id;
                cur.need_number = false;
                last_was_move = true;
            }
        }
    }
    if !stack.is_empty() {
        return Err("unclosed variation".into());
    }
    if !saw_result {
        return Err("no result token".into());
    }
    Ok(g)
}

fn shape(g: &Game) -> Vec<String> {
    let mut index = std::collections::HashMap::new();
    let mut out = Vec::new();
    index.insert(g.root(), 0usize);
    for (i, n) in g.descendants(g.root()).unwrap().enumerate() {
        index.insert(n, i + 1);
        if let Some(p) = g.parent(n).unwrap() {
            out.push(format!("{}>{:?}", index[&p], g.mv(n).unwrap()));
        }
    }
    out
}

fn has_null(g: &Game) -> bool {
    g.descendants(g.root())
        .unwrap()
        .any(|n| g.mv(n).is_ok_and(|m| m == Move::Null))
}

fn parse_tag_line(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix('[')?;
    let (name, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix('"')?;
    let mut val = String::new();
    let mut chars = rest.chars();
    loop {
        match chars.next()? {
            '\\' => match chars.next()? {
                '\\' => val.push('\\'),
                '"' => val.push('"'),
                _ => return None,
            },
            '"' => break,
            c => val.push(c),
        }
    }
    (chars.as_str() == "]").then(|| (name.to_owned(), val))
}

fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Wrapping invariants of a movetext block.
/// The indivisible units of a line: a move number stays with its move, and a
/// word starting with `%` stays with the word before it (rule 32 and the
/// "never at the start of a line" rule).
fn line_units(l: &str) -> usize {
    let mut units = 0usize;
    let mut glue_next = false;
    for (i, w) in l.trim_start().split(' ').enumerate() {
        let is_number = {
            let bare = w.trim_start_matches(['(', '{']);
            let digits = bare.chars().take_while(char::is_ascii_digit).count();
            digits > 0 && bare[digits..].chars().all(|c| c == '.') && bare.len() > digits
        };
        if i == 0 || !(glue_next || w.starts_with('%')) {
            units += 1;
        }
        glue_next = is_number;
    }
    units
}

/// Wrapping invariants of a movetext block.
fn check_wrapping(mt: &str, width: Option<usize>, check_percent: bool) -> Result<(), String> {
    for (i, l) in mt.lines().enumerate() {
        if l.is_empty() {
            return Err(format!("blank line {i} inside the movetext"));
        }
        if l.ends_with(' ') || l.ends_with('\t') {
            return Err(format!("trailing whitespace on line {i}: {l:?}"));
        }
        if check_percent && l.starts_with('%') {
            return Err(format!("line {i} starts with %: {l:?}"));
        }
        if let Some(w) = width {
            let n = l.chars().count();
            if n > w && line_units(l) > 1 {
                return Err(format!(
                    "line {i} has {n} > {w} chars and could wrap: {l:?}"
                ));
            }
        }
    }
    Ok(())
}

// ===========================================================================
// 1. Header
// ===========================================================================

mod header {
    use super::*;

    fn rich_game() -> Game {
        let mut g = fen_game("4k3/8/8/8/8/8/8/4K2R w K - 0 1");
        let t = g.tags_mut();
        t.event = "Candidates".into();
        t.site = "Paris FRA".into();
        t.date = PartialDate::new(Some(2024), Some(3), None).unwrap();
        t.round = "5.1".into();
        t.white = "Doe, John".into();
        t.black = "Roe, Rick".into();
        t.result = GameResult::Black;
        t.white_rating = Some(Rating::new(Elo::new(2700).unwrap(), RatingType::Elo));
        t.black_rating = Some(Rating::new(Elo::new(2650).unwrap(), RatingType::Rapid));
        t.eco = Some("C42".parse::<Eco>().unwrap());
        t.event_date = PartialDate::new(Some(2024), Some(3), Some(1)).unwrap();
        t.set_extra("Annotator", "Me").unwrap();
        t.set_extra("Source", "test").unwrap();
        g
    }

    #[test]
    fn default_game_writes_seven_tags_blank_line_and_result() {
        let text = render(&Game::new(), &WriteOptions::default());
        assert_eq!(
            text,
            "[Event \"?\"]\n[Site \"?\"]\n[Date \"????.??.??\"]\n[Round \"?\"]\n\
             [White \"?\"]\n[Black \"?\"]\n[Result \"*\"]\n\n*\n"
        );
    }

    #[test]
    fn full_header_orders_seven_ratings_eco_eventdate_extras_then_setup_fen() {
        let text = render(&rich_game(), &WriteOptions::default());
        let expected = "[Event \"Candidates\"]\n[Site \"Paris FRA\"]\n[Date \"2024.03.??\"]\n\
            [Round \"5.1\"]\n[White \"Doe, John\"]\n[Black \"Roe, Rick\"]\n[Result \"0-1\"]\n\
            [WhiteElo \"2700\"]\n[BlackRapid \"2650\"]\n[ECO \"C42\"]\n\
            [EventDate \"2024.03.01\"]\n[Annotator \"Me\"]\n[Source \"test\"]\n\
            [SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K2R w K - 0 1\"]\n\n0-1\n";
        assert_eq!(text, expected);
    }

    #[test]
    fn seven_tag_roster_set_keeps_only_seven_tags_plus_setup_and_fen() {
        let o = with(|o| o.tags = TagSet::SevenTagRoster);
        let text = render(&rich_game(), &o);
        let expected = "[Event \"Candidates\"]\n[Site \"Paris FRA\"]\n[Date \"2024.03.??\"]\n\
            [Round \"5.1\"]\n[White \"Doe, John\"]\n[Black \"Roe, Rick\"]\n[Result \"0-1\"]\n\
            [SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K2R w K - 0 1\"]\n\n0-1\n";
        assert_eq!(text, expected);
    }

    #[test]
    fn only_the_rating_that_exists_is_written() {
        let mut g = Game::new();
        g.tags_mut().black_rating = Some(Rating::new(Elo::new(1500).unwrap(), RatingType::Uscf));
        let text = render(&g, &WriteOptions::default());
        assert!(text.contains("[BlackUSCF \"1500\"]\n"), "{text}");
        assert!(
            !text.contains("[WhiteElo") && !text.contains("[WhiteUSCF"),
            "{text}"
        );
        assert_eq!(text.matches("1500").count(), 1, "{text}");
    }

    #[test]
    fn standard_start_position_writes_neither_setup_nor_fen() {
        let g = Game::from_position(Position::new());
        let text = render(&g, &WriteOptions::default());
        assert!(!text.contains("[SetUp"), "{text}");
        assert!(!text.contains("[FEN"), "{text}");
    }

    #[test]
    fn non_standard_start_with_black_to_move_writes_setup_then_fen_last() {
        let text = render(&fen_game(BLACK_TO_MOVE_FEN), &WriteOptions::default());
        assert!(
            text.contains(&format!(
                "[Result \"*\"]\n[SetUp \"1\"]\n[FEN \"{BLACK_TO_MOVE_FEN}\"]\n\n"
            )),
            "{text}"
        );
    }

    #[test]
    fn tag_values_escape_backslash_and_double_quote() {
        let mut g = Game::new();
        g.tags_mut().event = r#"Say "hi" \o/"#.into();
        g.tags_mut().set_extra("Note", r#"a"b\c\"#).unwrap();
        let text = render(&g, &WriteOptions::default());
        assert!(text.contains(r#"[Event "Say \"hi\" \\o/"]"#), "{text}");
        assert!(text.contains(r#"[Note "a\"b\\c\\"]"#), "{text}");
    }

    #[test]
    fn square_brackets_inside_values_are_not_escaped() {
        let mut g = Game::new();
        g.tags_mut().white = "x] [y".into();
        let text = render(&g, &WriteOptions::default());
        assert!(text.contains("[White \"x] [y\"]\n"), "{text}");
    }

    #[test]
    fn line_breaks_in_tag_values_become_spaces_and_keep_one_line_per_tag() {
        let mut g = Game::new();
        let t = g.tags_mut();
        t.event = "a\r\nb\nc\rd".into();
        t.site = "s1\ns2".into();
        t.round = "r\r1".into();
        t.white = "w\nw".into();
        t.black = "b\r\nb".into();
        let text = render(&g, &WriteOptions::default());
        let (head, _) = text.split_once("\n\n").unwrap();
        assert!(!head.contains('\r'), "{head:?}");
        assert_eq!(head.lines().count(), 7, "{head:?}");
        let (_, ev) = parse_tag_line(head.lines().next().unwrap()).unwrap();
        assert_eq!(norm_ws(&ev), "a b c d");
        for l in head.lines() {
            assert!(parse_tag_line(l).is_some(), "bad tag line {l:?}");
        }
    }

    #[test]
    fn long_tag_values_are_never_wrapped() {
        let mut g = Game::new();
        g.tags_mut().event = "word ".repeat(100).trim_end().to_owned();
        let o = with(|o| o.width = LineWidth::Columns(20));
        let text = render(&g, &o);
        let first = text.lines().next().unwrap();
        assert!(first.chars().count() > 400, "{first}");
        assert!(first.starts_with("[Event \"word word") && first.ends_with("word\"]"));
    }

    #[test]
    fn unicode_tag_values_pass_through_in_utf8() {
        let mut g = Game::new();
        g.tags_mut().white = "Zoë Ólafsdóttir 日本".into();
        let text = render(&g, &WriteOptions::default());
        assert!(text.contains("[White \"Zoë Ólafsdóttir 日本\"]\n"));
    }

    #[test]

    fn scid_flags_are_written_only_with_the_option_and_only_once() {
        let mut g = Game::new();
        g.tags_mut().event_date = PartialDate::new(Some(2020), None, None).unwrap();
        g.tags_mut().set_extra("Zeta", "z").unwrap();
        g.tags_mut().set_extra("ScidFlags", "UW").unwrap();
        let plain = render(&g, &WriteOptions::default());
        assert!(!plain.contains("ScidFlags"), "{plain}");
        let o = with(|o| o.scid_flags = true);
        let text = render(&g, &o);
        assert_eq!(text.matches("[ScidFlags").count(), 1, "{text}");
        // Scid order: ... EventDate, ScidFlags, other tags (game.cpp:3715-3750).
        let ev = text.find("[EventDate").unwrap();
        let fl = text.find("[ScidFlags \"UW\"]").unwrap();
        let ze = text.find("[Zeta").unwrap();
        assert!(ev < fl && fl < ze, "{text}");
    }

    #[test]
    fn empty_scid_flags_are_not_written() {
        let mut g = Game::new();
        g.tags_mut().set_extra("ScidFlags", "").unwrap();
        let text = render(&g, &with(|o| o.scid_flags = true));
        assert!(!text.contains("ScidFlags"), "{text}");
    }

    #[test]
    fn scid_flags_are_not_written_with_the_seven_tag_roster() {
        let mut g = Game::new();
        g.tags_mut().set_extra("ScidFlags", "UW").unwrap();
        let o = with(|o| {
            o.scid_flags = true;
            o.tags = TagSet::SevenTagRoster;
        });
        assert!(!render(&g, &o).contains("ScidFlags"));
    }

    #[rstest]
    #[case(GameResult::White, "1-0")]
    #[case(GameResult::Black, "0-1")]
    #[case(GameResult::Draw, "1/2-1/2")]
    #[case(GameResult::Unknown, "*")]
    fn result_tag_and_final_token_agree(#[case] r: GameResult, #[case] s: &str) {
        let mut g = sample_tree();
        g.tags_mut().result = r;
        let text = render(&g, &WriteOptions::default());
        assert!(text.contains(&format!("[Result \"{s}\"]\n")));
        assert!(text.ends_with(&format!(" {s}\n")), "{text}");
    }

    #[rstest]
    #[case(Some(2024), Some(3), Some(9), "2024.03.09")]
    #[case(Some(2024), Some(3), None, "2024.03.??")]
    #[case(Some(2024), None, None, "2024.??.??")]
    #[case(None, None, None, "????.??.??")]
    fn date_tag_uses_question_marks_for_unknown_parts(
        #[case] y: Option<u16>,
        #[case] m: Option<u8>,
        #[case] d: Option<u8>,
        #[case] expected: &str,
    ) {
        let mut g = Game::new();
        g.tags_mut().date = PartialDate::new(y, m, d).unwrap();
        assert!(render(&g, &WriteOptions::default()).contains(&format!("[Date \"{expected}\"]\n")));
    }

    fn short_game() -> Game {
        let mut g = Game::new();
        let t = g.tags_mut();
        t.event = "Wijk aan Zee".into();
        t.round = "5".into();
        t.site = "Amsterdam NED".into();
        t.date = PartialDate::new(Some(1999), Some(1), Some(20)).unwrap();
        t.white = "Kasparov, G".into();
        t.black = "Karpov, A".into();
        t.result = GameResult::White;
        t.white_rating = Some(Rating::new(Elo::new(2800).unwrap(), RatingType::Elo));
        t.black_rating = Some(Rating::new(Elo::new(2750).unwrap(), RatingType::Elo));
        t.eco = Some("B12".parse::<Eco>().unwrap());
        let root = g.root();
        play(&mut g, root, "e4");
        g
    }

    #[test]
    fn short_header_matches_scids_three_line_summary() {
        // game.cpp:3616-3690
        let o = with(|o| o.header = Header::Short);
        assert_eq!(
            render(&short_game(), &o),
            "Kasparov, G  (2800)   --   Karpov, A  (2750)\n\
             Wijk aan Zee  (Round 5)  Amsterdam NED  1999.01.20  1-0  B12\n\n1. e4 1-0\n"
        );
    }

    #[test]
    fn short_header_omits_unknown_round_site_eco_and_ratings() {
        let mut g = short_game();
        let t = g.tags_mut();
        t.round = "?".into();
        t.site = "?".into();
        t.eco = None;
        t.white_rating = None;
        t.black_rating = None;
        let o = with(|o| o.header = Header::Short);
        assert_eq!(
            render(&g, &o),
            "Kasparov, G   --   Karpov, A\nWijk aan Zee  1999.01.20  1-0\n\n1. e4 1-0\n"
        );
    }

    #[rstest]
    #[case(Some(1999), None, None, "1999")]
    #[case(Some(1999), Some(3), None, "1999.03")]
    #[case(None, None, None, "????")]
    fn short_header_trims_unknown_date_suffix(
        #[case] y: Option<u16>,
        #[case] m: Option<u8>,
        #[case] d: Option<u8>,
        #[case] expected: &str,
    ) {
        let mut g = short_game();
        g.tags_mut().date = PartialDate::new(y, m, d).unwrap();
        let o = with(|o| o.header = Header::Short);
        let text = render(&g, &o);
        let second = text.lines().nth(1).unwrap();
        assert!(second.contains(&format!("  {expected}  1-0")), "{second:?}");
    }

    #[test]
    fn short_header_has_position_line_for_fen_start_and_no_tag_lines() {
        let o = with(|o| o.header = Header::Short);
        let text = render(&fen_game(BLACK_TO_MOVE_FEN), &o);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[2], format!("Position: {BLACK_TO_MOVE_FEN}"), "{text}");
        assert!(!text.contains('['), "{text}");
        assert_eq!(lines[3], "", "{text}");
    }
}

// ===========================================================================
// 2. Movetext
// ===========================================================================

mod movetext_tests {
    use super::*;

    #[test]
    fn variation_after_the_main_move_gets_its_own_black_number() {
        assert_eq!(
            mt_default(&sample_tree()),
            "1. e4 e5 (1... c5 2. Nf3) 2. Nf3 *\n"
        );
    }

    #[test]
    fn nested_variations_close_in_order_and_renumber_black_moves() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let e5 = play(&mut g, e4, "e5");
        let nf3 = play(&mut g, e5, "Nf3");
        let nc6 = play(&mut g, nf3, "Nc6");
        let nf6 = play(&mut g, nf3, "Nf6");
        let nxe5 = play(&mut g, nf6, "Nxe5");
        play(&mut g, nxe5, "d6");
        line(&mut g, nf6, "Nc3 Nxe4");
        play(&mut g, nc6, "Bb5");
        assert_eq!(
            mt_default(&g),
            "1. e4 e5 2. Nf3 Nc6 (2... Nf6 3. Nxe5 (3. Nc3 Nxe4) 3... d6) 3. Bb5 *\n"
        );
    }

    #[test]
    fn white_variation_forces_a_number_on_the_following_black_main_move() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let e5 = play(&mut g, e4, "e5");
        let nf3 = play(&mut g, e5, "Nf3");
        play(&mut g, nf3, "Nc6");
        play(&mut g, e5, "Nc3");
        assert_eq!(mt_default(&g), "1. e4 e5 2. Nf3 (2. Nc3) 2... Nc6 *\n");
    }

    #[test]
    fn two_variations_in_a_row_are_written_in_children_order() {
        let mut g = Game::new();
        let root = g.root();
        play(&mut g, root, "e4");
        play(&mut g, root, "d4");
        play(&mut g, root, "c4");
        assert_eq!(mt_default(&g), "1. e4 (1. d4) (1. c4) *\n");
    }

    #[test]
    fn promoting_a_variation_swaps_the_order_in_the_output() {
        let mut g = Game::new();
        let root = g.root();
        play(&mut g, root, "e4");
        let d4 = play(&mut g, root, "d4");
        g.promote_variation(d4).unwrap();
        assert_eq!(mt_default(&g), "1. d4 (1. e4) *\n");
    }

    #[test]
    fn game_starting_with_black_to_move_uses_the_ellipsis_number() {
        let mut g = fen_game(BLACK_TO_MOVE_FEN);
        let root = g.root();
        line(&mut g, root, "e5 Nf3");
        assert_eq!(mt_default(&g), "1... e5 2. Nf3 *\n");
        let o = with(|o| o.move_number_space = false);
        assert_eq!(movetext(&g, &o), "1...e5 2.Nf3 *\n");
    }

    #[test]
    fn fen_start_uses_the_fullmove_number_of_the_position() {
        let mut g = fen_game("4k3/8/8/8/8/8/8/4K2R b K - 0 17");
        let root = g.root();
        line(&mut g, root, "Kd7 Rh7+");
        assert_eq!(mt_default(&g), "17... Kd7 18. Rh7+ *\n");
    }

    #[test]
    fn variation_in_a_black_to_move_start_begins_with_an_ellipsis_number() {
        let mut g = fen_game(BLACK_TO_MOVE_FEN);
        let root = g.root();
        play(&mut g, root, "e5");
        play(&mut g, root, "c5");
        assert_eq!(mt_default(&g), "1... e5 (1... c5) *\n");
    }

    #[test]
    fn move_number_space_off_glues_numbers_to_moves() {
        let o = with(|o| o.move_number_space = false);
        assert_eq!(
            movetext(&sample_tree(), &o),
            "1.e4 e5 (1...c5 2.Nf3) 2.Nf3 *\n"
        );
    }

    #[test]
    fn black_move_after_a_comment_gets_a_number() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "best").unwrap();
        line(&mut g, e4, "e5 Nf3");
        assert_eq!(mt_default(&g), "1. e4 {best} 1... e5 2. Nf3 *\n");
    }

    #[test]
    fn white_move_after_a_comment_keeps_its_number_and_no_extra_one() {
        let mut g = Game::new();
        let root = g.root();
        let e5 = line(&mut g, root, "e4 e5");
        g.set_comment(e5, "sym").unwrap();
        play(&mut g, e5, "Nf3");
        assert_eq!(mt_default(&g), "1. e4 e5 {sym} 2. Nf3 *\n");
    }

    #[test]
    fn initial_comment_goes_before_the_first_move_and_then_breaks_the_line() {
        let mut g = Game::new();
        g.set_initial_comment("Opening note");
        let root = g.root();
        play(&mut g, root, "e4");
        assert_eq!(mt_default(&g), "{Opening note}\n1. e4 *\n");
    }

    #[test]
    fn initial_comment_before_a_black_first_move_is_followed_by_the_ellipsis() {
        let mut g = fen_game(BLACK_TO_MOVE_FEN);
        g.set_initial_comment("hi");
        let root = g.root();
        play(&mut g, root, "e5");
        assert_eq!(mt_default(&g), "{hi}\n1... e5 *\n");
    }

    #[test]
    fn game_with_only_an_initial_comment_still_ends_with_the_result() {
        let mut g = Game::new();
        g.set_initial_comment("just a note");
        let mt = mt_default(&g);
        assert!(mt.starts_with("{just a note}"), "{mt:?}");
        assert!(mt.ends_with("*\n") && !mt.contains("\n\n"), "{mt:?}");
    }

    #[test]
    fn comment_before_a_variation_is_written_right_after_the_open_parenthesis() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        play(&mut g, e4, "e5");
        let c5 = play(&mut g, e4, "c5");
        g.set_comment_before(c5, "sicilian").unwrap();
        assert_eq!(mt_default(&g), "1. e4 e5 ({sicilian} 1... c5) *\n");
    }

    #[test]
    fn comment_after_a_variation_move_then_black_number() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        play(&mut g, e4, "e5");
        let nf3 = line(&mut g, e4, "c5 Nf3");
        g.set_comment(nf3, "inside").unwrap();
        play(&mut g, nf3, "d6");
        assert_eq!(
            mt_default(&g),
            "1. e4 e5 (1... c5 2. Nf3 {inside} 2... d6) *\n"
        );
    }

    #[test]
    fn comment_text_has_no_padding_inside_the_braces() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "  padded   text  ").unwrap();
        let mt = mt_default(&g);
        assert!(mt.contains("{padded") && mt.contains("text}"), "{mt:?}");
        assert!(!mt.contains("  "), "double space in {mt:?}");
    }

    #[test]
    fn comments_turned_off_drop_comments_but_not_nags() {
        let mut g = Game::new();
        g.set_initial_comment("start");
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "c1").unwrap();
        g.add_nag(e4, nag(1)).unwrap();
        let e5 = play(&mut g, e4, "e5");
        g.set_comment_before(e5, "never shown").unwrap();
        let o = with(|o| o.comments = false);
        // D32(e): NAGs are independent of `comments`.
        assert_eq!(movetext(&g, &o), "1. e4 $1 e5 *\n");
    }

    #[test]
    fn comments_turned_off_still_number_black_moves_after_the_hidden_comment_correctly() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "hidden").unwrap();
        play(&mut g, e4, "e5");
        let o = with(|o| o.comments = false);
        assert_eq!(movetext(&g, &o), "1. e4 e5 *\n");
    }

    #[test]
    fn variations_turned_off_write_only_the_main_line() {
        let o = with(|o| o.variations = false);
        assert_eq!(movetext(&sample_tree(), &o), "1. e4 e5 2. Nf3 *\n");
    }

    #[test]
    fn variations_turned_off_do_not_force_numbers_on_following_black_moves() {
        let mut g = Game::new();
        let root = g.root();
        let e5 = line(&mut g, root, "e4 e5");
        let nf3 = play(&mut g, e5, "Nf3");
        play(&mut g, nf3, "Nc6");
        play(&mut g, e5, "Nc3");
        let o = with(|o| o.variations = false);
        assert_eq!(movetext(&g, &o), "1. e4 e5 2. Nf3 Nc6 *\n");
    }

    #[test]
    fn variations_turned_off_also_drop_variation_comments() {
        let mut g = Game::new();
        let root = g.root();
        play(&mut g, root, "e4");
        let d4 = play(&mut g, root, "d4");
        g.set_comment(d4, "alt").unwrap();
        g.set_comment_before(d4, "before").unwrap();
        let o = with(|o| o.variations = false);
        assert_eq!(movetext(&g, &o), "1. e4 *\n");
    }

    #[test]
    fn empty_game_writes_only_the_result_token() {
        assert_eq!(mt_default(&Game::new()), "*\n");
    }

    // ---- comments ----

    fn game_with_comment(text: &str) -> Game {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, text).unwrap();
        g
    }

    #[rstest]
    #[case("a {b} c", "{a (b) c}")]
    #[case("x}y", "{x)y}")]
    #[case("{{", "{((}")]
    #[case("}", "{)}")]
    fn braces_inside_comments_become_parentheses_by_default(
        #[case] input: &str,
        #[case] shown: &str,
    ) {
        let mt = mt_default(&game_with_comment(input));
        assert_eq!(mt, format!("1. e4 {shown} *\n"));
    }

    #[rstest]
    #[case("a {b} c", "{a b c}")]
    #[case("x}y", "{xy}")]
    #[case("{{ z }}", "{z}")]
    fn braces_inside_comments_are_deleted_with_strip(#[case] input: &str, #[case] shown: &str) {
        let o = with(|o| o.braces = BraceHandling::Strip);
        let mt = movetext(&game_with_comment(input), &o);
        // Whitespace normalisation is not part of this rule: compare words.
        assert_eq!(norm_ws(&mt), norm_ws(&format!("1. e4 {shown} *\n")));
    }

    #[test]
    fn comment_that_is_only_braces_is_dropped_with_strip() {
        let o = with(|o| o.braces = BraceHandling::Strip);
        assert_eq!(movetext(&game_with_comment("{}"), &o), "1. e4 *\n");
        assert_eq!(movetext(&game_with_comment("}{"), &o), "1. e4 *\n");
    }

    #[test]
    fn writer_never_emits_a_literal_brace_inside_a_comment() {
        for braces in [BraceHandling::ReplaceWithParens, BraceHandling::Strip] {
            let o = with(|o| o.braces = braces);
            let mt = movetext(&game_with_comment("a { b } c }{ d"), &o);
            paren_depth(&mt).unwrap();
            assert_eq!(mt.matches('{').count(), 1, "{mt:?}");
            assert_eq!(mt.matches('}').count(), 1, "{mt:?}");
        }
    }

    #[test]
    fn strip_marks_removes_commands_and_keeps_the_text() {
        let o = with(|o| o.strip_marks = true);
        let mt = movetext(
            &game_with_comment("[%clk 0:01:00] good [%csl Gd4] move"),
            &o,
        );
        assert!(mt.starts_with("1. e4 {"), "{mt:?}");
        assert!(!mt.contains("[%"), "{mt:?}");
        assert!(mt.contains("good") && mt.contains("move"), "{mt:?}");
        assert_eq!(
            movetext(&game_with_comment("[%clk 0:01:00] good"), &o),
            "1. e4 {good} *\n"
        );
    }

    #[test]
    fn strip_marks_drops_a_comment_that_becomes_empty() {
        let o = with(|o| o.strip_marks = true);
        assert_eq!(
            movetext(&game_with_comment("[%clk 0:01:00]"), &o),
            "1. e4 *\n"
        );
        assert_eq!(
            movetext(&game_with_comment("[%csl Gd4][%cal Ge2e4]"), &o),
            "1. e4 *\n"
        );
    }

    #[test]
    fn strip_marks_empty_initial_and_variation_comments_are_dropped() {
        let mut g = Game::new();
        g.set_initial_comment("[%clk 1:00:00]");
        let root = g.root();
        play(&mut g, root, "e4");
        let d4 = play(&mut g, root, "d4");
        g.set_comment_before(d4, "[%clk 0:59:00]").unwrap();
        g.set_comment(d4, "[%eval 0.3]").unwrap();
        let o = with(|o| o.strip_marks = true);
        assert_eq!(movetext(&g, &o), "1. e4 (1. d4) *\n");
    }

    #[test]
    fn dropped_empty_comment_does_not_force_a_black_number() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "[%clk 0:01:00]").unwrap();
        play(&mut g, e4, "e5");
        let o = with(|o| o.strip_marks = true);
        assert_eq!(movetext(&g, &o), "1. e4 e5 *\n");
    }

    #[test]
    fn strip_marks_still_replaces_braces_so_the_output_stays_balanced() {
        // Scid's WriteComment skips the brace conversion when strip_marks is on.
        let o = with(|o| o.strip_marks = true);
        let mt = movetext(&game_with_comment("[%clk 1:00] a {b} c"), &o);
        paren_depth(&mt).unwrap();
        assert_eq!(mt, "1. e4 {a (b) c} *\n");
    }

    #[test]
    fn marks_are_kept_without_strip_marks() {
        let mt = mt_default(&game_with_comment("[%clk 0:01:00] ok"));
        assert_eq!(mt, "1. e4 {[%clk 0:01:00] ok} *\n");
    }

    #[test]
    fn explicit_newlines_of_a_comment_are_kept() {
        let mt = mt_default(&game_with_comment("line one\nline two"));
        assert!(mt.contains("{line one\nline two}"), "{mt:?}");
    }

    #[test]
    fn comment_with_blank_lines_never_creates_a_blank_line_in_the_movetext() {
        // D31: "sem linhas em branco extras dentro do movetext".
        let mt = mt_default(&game_with_comment("a\n\nb\n\n\nc"));
        assert!(!mt.contains("\n\n"), "{mt:?}");
        paren_depth(&mt).unwrap();
    }

    #[test]
    fn comment_line_starting_with_percent_is_never_left_at_column_zero() {
        let mt = mt_default(&game_with_comment("x\n%y z"));
        assert!(mt.lines().all(|l| !l.starts_with('%')), "{mt:?}");
    }

    #[test]
    fn comment_line_starting_with_a_tag_bracket_stays_inside_balanced_braces() {
        // Observation: rule 6 only treats `[` at the start of a line as a new
        // game *outside* comments, so the line is kept as is; the braces must
        // still enclose it.
        let mt = mt_default(&game_with_comment("x\n[Event \"y\"]"));
        paren_depth(&mt).unwrap();
        assert_eq!(mt, "1. e4 {x\n[Event \"y\"]} *\n");
    }

    // ---- NAGs ----

    fn game_with_nags(nags: &[u16]) -> Game {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        for &n in nags {
            g.add_nag(e4, nag(n)).unwrap();
        }
        g
    }

    #[rstest]
    #[case(&[], "1. e4 *\n")]
    #[case(&[1], "1. e4 $1 *\n")]
    #[case(&[1, 14], "1. e4 $1 $14 *\n")]
    #[case(&[255, 7, 1], "1. e4 $255 $7 $1 *\n")]
    fn numeric_nags_follow_the_move_separated_by_spaces(
        #[case] nags: &[u16],
        #[case] expected: &str,
    ) {
        assert_eq!(mt_default(&game_with_nags(nags)), expected);
    }

    #[test]
    fn nags_none_writes_no_nag() {
        let o = with(|o| o.nags = NagOutput::None);
        assert_eq!(movetext(&game_with_nags(&[1, 14, 7]), &o), "1. e4 *\n");
    }

    #[rstest]
    #[case(&[1], "1. e4! *\n")]
    #[case(&[2], "1. e4? *\n")]
    #[case(&[3], "1. e4!! *\n")]
    #[case(&[4], "1. e4?? *\n")]
    #[case(&[5], "1. e4!? *\n")]
    #[case(&[6], "1. e4?! *\n")]
    #[case(&[14], "1. e4 += *\n")]
    #[case(&[18], "1. e4 +- *\n")]
    #[case(&[1, 14], "1. e4! += *\n")]
    #[case(&[1, 2], "1. e4! ? *\n")]
    #[case(&[7], "1. e4 $7 *\n")]
    #[case(&[1, 7, 14], "1. e4! $7 += *\n")]
    #[case(&[142], "1. e4 $142 *\n")]
    #[case(&[210], "1. e4 $210 *\n")]
    fn symbolic_nags_glue_the_first_mark_and_fall_back_to_numbers(
        #[case] nags: &[u16],
        #[case] expected: &str,
    ) {
        let o = with(|o| o.nags = NagOutput::Symbolic);
        assert_eq!(movetext(&game_with_nags(nags), &o), expected);
    }

    #[test]
    fn symbolic_nag_that_is_not_a_mark_is_not_glued_even_when_first() {
        let o = with(|o| o.nags = NagOutput::Symbolic);
        let mt = movetext(&game_with_nags(&[14, 1]), &o);
        // The first NAG is "+=": it is written separated; "!" is the first mark.
        assert!(mt.starts_with("1. e4"), "{mt:?}");
        assert!(mt.contains(" += ") && mt.contains('!'), "{mt:?}");
    }

    #[test]
    fn every_symbolic_nag_is_reread_as_itself_or_written_numerically() {
        // D33: only symbols the reader understands; anything that could be
        // confused with PGN syntax (parentheses, `--`, ...) stays `$n`.
        let o = with(|o| o.nags = NagOutput::Symbolic);
        for n in 1..=255u16 {
            let mt = movetext(&game_with_nags(&[n]), &o);
            let body = mt
                .strip_prefix("1. e4")
                .and_then(|r| r.strip_suffix(" *\n"))
                .unwrap_or_else(|| panic!("NAG {n}: unexpected shape {mt:?}"));
            let sym = body.trim();
            if sym == format!("${n}") {
                continue;
            }
            assert_eq!(Nag::parse(sym), Some(nag(n)), "NAG {n} written as {sym:?}");
            assert!(
                !sym.contains(['(', ')', '{', '}', ';', '%']) && sym != "--",
                "NAG {n} written as dangerous symbol {sym:?}"
            );
        }
    }

    #[test]
    fn nag_precedes_the_comment_of_the_same_move() {
        // Observation (implementer's note): `2. Nf3 $1 {bom}`.
        let mut g = Game::new();
        let root = g.root();
        let nf3 = line(&mut g, root, "e4 e5 Nf3");
        g.add_nag(nf3, nag(1)).unwrap();
        g.set_comment(nf3, "bom").unwrap();
        assert_eq!(mt_default(&g), "1. e4 e5 2. Nf3 $1 {bom} *\n");
    }

    #[test]
    fn nag_goes_before_the_variations_of_its_move() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.add_nag(e4, nag(1)).unwrap();
        play(&mut g, root, "d4");
        assert_eq!(mt_default(&g), "1. e4 $1 (1. d4) *\n");
    }

    // ---- null moves ----

    fn null_game() -> (Game, NodeId) {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let null = g.add_move(e4, Move::Null).unwrap();
        (g, null)
    }

    #[test]
    fn null_move_is_kept_by_default() {
        let (mut g, null) = null_game();
        play(&mut g, null, "d4");
        assert_eq!(mt_default(&g), "1. e4 -- 2. d4 *\n");
    }

    #[test]
    fn white_null_move_at_the_root_is_numbered() {
        let mut g = Game::new();
        let root = g.root();
        let null = g.add_move(root, Move::Null).unwrap();
        line(&mut g, null, "e5 Nf3");
        assert_eq!(mt_default(&g), "1. -- e5 2. Nf3 *\n");
    }

    #[test]
    fn kept_null_move_after_a_comment_gets_an_ellipsis_number() {
        let (mut g, null) = null_game();
        let e4 = g.parent(null).unwrap().unwrap();
        g.set_comment(e4, "pass").unwrap();
        assert_eq!(mt_default(&g), "1. e4 {pass} 1... -- *\n");
    }

    #[test]
    fn convert_to_comment_puts_the_rest_of_the_line_in_one_comment() {
        let (mut g, null) = null_game();
        line(&mut g, null, "d4 d5");
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        assert_eq!(movetext(&g, &o), "1. e4 {-- 2. d4 d5} *\n");
    }

    #[test]
    fn convert_to_comment_without_moves_after_the_null() {
        let (g, _) = null_game();
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        assert_eq!(movetext(&g, &o), "1. e4 {--} *\n");
    }

    #[test]
    fn convert_to_comment_keeps_nested_variations_inside_the_comment() {
        let (mut g, null) = null_game();
        let d4 = play(&mut g, null, "d4");
        play(&mut g, d4, "d5");
        line(&mut g, null, "c4 c5");
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        let mt = movetext(&g, &o);
        paren_depth(&mt).unwrap();
        assert_eq!(mt.matches('{').count(), 1, "{mt:?}");
        assert_eq!(mt, "1. e4 {-- 2. d4 (2. c4 c5) 2... d5} *\n");
    }

    #[test]
    fn convert_to_comment_never_nests_braces_when_the_region_has_comments() {
        let (mut g, null) = null_game();
        let d4 = play(&mut g, null, "d4");
        g.set_comment(d4, "note {x}").unwrap();
        g.set_comment_before(d4, "pre").unwrap();
        let d5 = play(&mut g, d4, "d5");
        g.set_comment(d5, "after").unwrap();
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        let mt = movetext(&g, &o);
        assert_eq!(paren_depth(&mt), Ok(0), "{mt:?}");
        assert_eq!(mt.matches('{').count(), 1, "{mt:?}");
        assert_eq!(mt.matches('}').count(), 1, "{mt:?}");
        assert!(mt.contains("note") && mt.contains("after"), "{mt:?}");
    }

    #[test]
    fn convert_to_comment_inside_a_variation_only_affects_that_variation() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        play(&mut g, e4, "e5");
        let nf3 = line(&mut g, e4, "c5 Nf3");
        let null = g.add_move(nf3, Move::Null).unwrap();
        play(&mut g, null, "d4");
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        let mt = movetext(&g, &o);
        assert_eq!(mt, "1. e4 e5 (1... c5 2. Nf3 {-- 3. d4}) *\n");
    }

    #[test]
    fn convert_to_comment_does_not_touch_main_line_after_a_null_in_a_variation() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let e5 = play(&mut g, e4, "e5");
        let nf3 = line(&mut g, e4, "c5 Nf3");
        g.add_move(nf3, Move::Null).unwrap();
        line(&mut g, e5, "Nf3 Nc6");
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        let mt = movetext(&g, &o);
        assert!(mt.ends_with(") 2. Nf3 Nc6 *\n"), "{mt:?}");
    }

    #[test]
    fn convert_to_comment_with_a_white_null_move_keeps_the_number_inside() {
        let mut g = Game::new();
        let root = g.root();
        let null = g.add_move(root, Move::Null).unwrap();
        play(&mut g, null, "e5");
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        let mt = movetext(&g, &o);
        assert_eq!(mt, "{1. -- e5} *\n");
    }

    #[test]
    fn convert_to_comment_leaves_games_without_null_moves_untouched() {
        let o = with(|o| o.null_moves = NullMoves::ConvertToComment);
        assert_eq!(movetext(&sample_tree(), &o), mt_default(&sample_tree()));
    }

    // ---- indentation ----

    #[test]
    fn indent_variations_put_each_variation_on_an_indented_line() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let e5 = play(&mut g, e4, "e5");
        let nf3 = play(&mut g, e5, "Nf3");
        let nc6 = play(&mut g, nf3, "Nc6");
        let nf6 = play(&mut g, nf3, "Nf6");
        let nxe5 = play(&mut g, nf6, "Nxe5");
        play(&mut g, nxe5, "d6");
        line(&mut g, nf6, "Nc3 Nxe4");
        play(&mut g, nc6, "Bb5");
        let o = with(|o| o.indent_variations = true);
        assert_eq!(
            movetext(&g, &o),
            "1. e4 e5 2. Nf3 Nc6\n    (2... Nf6 3. Nxe5\n        (3. Nc3 Nxe4)\n    3... d6)\n3. Bb5 *\n"
        );
    }

    #[test]
    fn indent_variations_separate_consecutive_variations_on_their_own_lines() {
        let mut g = Game::new();
        let root = g.root();
        play(&mut g, root, "e4");
        play(&mut g, root, "d4");
        play(&mut g, root, "c4");
        let o = with(|o| o.indent_variations = true);
        let mt = movetext(&g, &o);
        assert_eq!(mt, "1. e4\n    (1. d4)\n    (1. c4)\n*\n");
    }

    #[test]
    fn indent_comments_move_main_line_comments_to_their_own_indented_line() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "good").unwrap();
        let e5 = play(&mut g, e4, "e5");
        let c5 = play(&mut g, e4, "c5");
        g.set_comment(c5, "var comment").unwrap();
        play(&mut g, e5, "Nf3");
        let o = with(|o| o.indent_comments = true);
        let mt = movetext(&g, &o);
        let lines: Vec<&str> = mt.lines().collect();
        assert_eq!(lines[0], "1. e4", "{mt:?}");
        assert_eq!(lines[1], "    {good}", "{mt:?}");
        assert!(
            lines[2].starts_with("1... e5 (1... c5 {var comment}"),
            "{mt:?}"
        );
    }

    #[test]
    fn indent_comments_and_indent_variations_combine() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "c").unwrap();
        play(&mut g, e4, "e5");
        play(&mut g, root, "d4");
        let o = with(|o| {
            o.indent_comments = true;
            o.indent_variations = true;
        });
        let mt = movetext(&g, &o);
        paren_depth(&mt).unwrap();
        assert!(mt.contains("    {c}\n"), "{mt:?}");
        assert!(mt.contains("\n    (1. d4)\n"), "{mt:?}");
    }

    #[test]
    fn column_style_forces_indentation_options() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "c").unwrap();
        play(&mut g, root, "d4");
        let col = with(|o| o.column = true);
        let both = with(|o| {
            o.column = true;
            o.indent_comments = true;
            o.indent_variations = true;
        });
        assert_eq!(movetext(&g, &col), movetext(&g, &both));
    }

    #[test]
    fn column_style_writes_one_full_move_per_row_with_tab_separated_columns() {
        // D35 / rule 34. A leading tab is Scid's `startColumn` (game.cpp:3217).
        let mut g = Game::new();
        let root = g.root();
        line(&mut g, root, "e4 e5 Nf3 Nc6");
        let o = with(|o| o.column = true);
        let mt = movetext(&g, &o);
        let rows: Vec<&str> = mt.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(rows.len(), 3, "{mt:?}");
        assert_eq!(rows[0].trim_start_matches('\t'), "   1.\te4          \te5");
        assert_eq!(rows[1].trim_start_matches('\t'), "   2.\tNf3         \tNc6");
        assert_eq!(rows[2], "*");
    }

    #[test]
    fn column_style_pads_the_white_move_counting_its_nags() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.add_nag(e4, nag(1)).unwrap();
        play(&mut g, e4, "e5");
        let o = with(|o| {
            o.column = true;
            o.nags = NagOutput::Symbolic;
        });
        let mt = movetext(&g, &o);
        let row = mt.lines().find(|l| l.contains("e4")).unwrap();
        assert_eq!(
            row.trim_start_matches('\t'),
            "   1.\te4!         \te5",
            "{mt:?}"
        );
    }

    #[test]
    fn column_style_black_row_after_a_comment_restarts_with_the_ellipsis() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "hm").unwrap();
        play(&mut g, e4, "e5");
        let o = with(|o| o.column = true);
        let mt = movetext(&g, &o);
        assert!(mt.contains("{hm}"), "{mt:?}");
        let row = mt.lines().find(|l| l.contains("e5")).unwrap();
        assert!(
            row.contains("...") && row.trim_end().ends_with("e5"),
            "{row:?}"
        );
        assert!(mt.trim_end().ends_with('*'));
    }

    // ---- results ----

    #[test]
    fn final_result_shares_the_last_line_when_it_fits() {
        let mut g = sample_tree();
        g.tags_mut().result = GameResult::Draw;
        assert_eq!(mt_default(&g), "1. e4 e5 (1... c5 2. Nf3) 2. Nf3 1/2-1/2\n");
    }
}

// ===========================================================================
// 3. Layout: width, newlines
// ===========================================================================

mod layout {
    use super::*;

    #[rstest]
    fn no_line_exceeds_the_width_and_none_ends_with_space(
        #[values(10, 20, 40, 75, 80, 120)] width: u16,
        #[values(0, 1, 2)] style: u8,
    ) {
        let g = busy_game(7);
        let o = with(|o| {
            o.width = LineWidth::Columns(width);
            o.indent_variations = style == 1;
            o.indent_comments = style == 2;
        });
        let mt = movetext(&g, &o);
        check_wrapping(&mt, Some(usize::from(width)), true).unwrap_or_else(|e| panic!("{e}\n{mt}"));
        paren_depth(&mt).unwrap();
    }

    #[test]
    fn scid_export_preset_wraps_at_75() {
        let g = busy_game(11);
        let o = WriteOptions::scid_export();
        let text = game_to_string(&g, &o).unwrap();
        let mt = text.split_once("\n\n").unwrap().1;
        check_wrapping(mt, Some(75), true).unwrap();
        assert!(
            mt.lines().any(|l| l.chars().count() > 70),
            "wrapped too early:\n{mt}"
        );
    }

    #[rstest]
    fn plain_mainline_wraps_greedily_up_to_exactly_the_width(
        #[values(12, 20, 33, 40, 60, 75, 80, 100)] width: u16,
    ) {
        // Rule 32: nothing passes `width`, and a line of exactly `width`
        // characters is allowed, so plain words fill each line greedily.
        let mut g = Game::new();
        let root = g.root();
        let all = std::iter::repeat_n("Nf3 Nf6 Ng1 Ng8", 60)
            .collect::<Vec<_>>()
            .join(" ");
        line(&mut g, root, &all);
        // Words of the output: "N. move" for white, "move" for black, then "*".
        let cycle = ["Nf3", "Nf6", "Ng1", "Ng8"];
        let mut words: Vec<String> = (0..240)
            .map(|i| {
                if i % 2 == 0 {
                    format!("{}. {}", i / 2 + 1, cycle[i % 4])
                } else {
                    cycle[i % 4].to_owned()
                }
            })
            .collect();
        words.push("*".to_owned());
        let width = usize::from(width);
        let mut expected = String::new();
        let mut col = 0;
        for w in &words {
            let n = w.chars().count();
            if col == 0 {
                expected.push_str(w);
                col = n;
            } else if col + 1 + n <= width {
                expected.push(' ');
                expected.push_str(w);
                col += 1 + n;
            } else {
                expected.push('\n');
                expected.push_str(w);
                col = n;
            }
        }
        expected.push('\n');
        let o = with(|o| o.width = LineWidth::Columns(width as u16));
        assert_eq!(movetext(&g, &o), expected);
    }

    #[test]
    fn zero_width_is_treated_as_one_and_does_not_panic() {
        let g = busy_game(3);
        let zero = movetext(&g, &with(|o| o.width = LineWidth::Columns(0)));
        let one = movetext(&g, &with(|o| o.width = LineWidth::Columns(1)));
        assert_eq!(zero, one);
        check_wrapping(&zero, Some(1), true).unwrap();
        paren_depth(&zero).unwrap();
    }

    #[test]
    fn unlimited_width_keeps_the_movetext_on_one_line() {
        let mut g = Game::new();
        let root = g.root();
        let all = std::iter::repeat_n("Nf3 Nf6 Ng1 Ng8", 100)
            .collect::<Vec<_>>()
            .join(" ");
        let last = line(&mut g, root, &all);
        g.set_comment(
            last,
            "a long comment with many words in it that would wrap at eighty columns for sure",
        )
        .unwrap();
        let o = with(|o| o.width = LineWidth::Unlimited);
        let mt = movetext(&g, &o);
        assert_eq!(mt.lines().count(), 1, "{mt}");
        assert!(
            mt.ends_with("for sure} 1... Ng8 *\n") || mt.contains("for sure}"),
            "{mt}"
        );
    }

    #[test]
    fn percent_words_never_start_a_line_at_any_wrap_position() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(
            e4,
            "a %b %c %d %e %f %g %h %i %j %k %l %m %n %o %p %q %r %s %t",
        )
        .unwrap();
        for width in 3..=60u16 {
            let o = with(|o| o.width = LineWidth::Columns(width));
            let mt = movetext(&g, &o);
            for l in mt.lines() {
                assert!(!l.starts_with('%'), "width {width}: {l:?} in\n{mt}");
            }
        }
    }

    #[test]
    fn percent_words_in_variation_comments_with_indentation_do_not_lead_a_line() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let d4 = play(&mut g, root, "d4");
        g.set_comment(d4, "%x %y %z %w %v").unwrap();
        g.set_comment(e4, "%a %b %c %d").unwrap();
        for width in 4..=40u16 {
            let o = with(|o| {
                o.width = LineWidth::Columns(width);
                o.indent_variations = true;
                o.indent_comments = true;
            });
            let mt = movetext(&g, &o);
            assert!(
                mt.lines().all(|l| !l.starts_with('%')),
                "width {width}:\n{mt}"
            );
        }
    }

    #[test]
    fn indivisible_word_longer_than_the_width_stays_whole_on_its_own_line() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let long = "x".repeat(100);
        g.set_comment(e4, &format!("before {long} after")).unwrap();
        let mt = movetext(&g, &with(|o| o.width = LineWidth::Columns(30)));
        assert!(mt.contains(&long), "word was split: {mt}");
        check_wrapping(&mt, Some(30), true).unwrap();
    }

    #[test]
    fn width_counts_characters_not_bytes() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, &"\u{e9}\u{e9}\u{e9} ".repeat(40))
            .unwrap();
        let mt = movetext(&g, &with(|o| o.width = LineWidth::Columns(20)));
        check_wrapping(&mt, Some(20), true).unwrap();
        let max = mt.lines().map(|l| l.chars().count()).max().unwrap();
        assert!(max >= 17, "lines too short (counting bytes?): {mt}");
    }

    #[test]
    fn wide_unicode_text_is_wrapped_by_character_count() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, &"\u{65e5}\u{672c}\u{8a9e} ".repeat(30))
            .unwrap();
        let mt = movetext(&g, &with(|o| o.width = LineWidth::Columns(25)));
        check_wrapping(&mt, Some(25), true).unwrap();
    }

    #[test]
    fn crlf_is_used_everywhere_including_explicit_comment_breaks() {
        let mut g = busy_game(5);
        let root = g.root();
        let n = play(&mut g, root, "a3");
        g.set_comment(n, "first\nsecond").unwrap();
        for pgn_header in [Header::Pgn, Header::Short] {
            let o = with(|o| {
                o.newline = Newline::CrLf;
                o.header = pgn_header;
            });
            let text = render(&g, &o);
            let bytes = text.as_bytes();
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'\n' {
                    assert!(i > 0 && bytes[i - 1] == b'\r', "bare LF at {i}");
                }
                if b == b'\r' {
                    assert_eq!(bytes.get(i + 1), Some(&b'\n'), "bare CR at {i}");
                }
            }
            assert!(text.ends_with("\r\n"));
        }
    }

    #[test]
    fn lf_output_contains_no_carriage_return() {
        let text = render(&busy_game(9), &WriteOptions::default());
        assert!(!text.contains('\r'));
    }

    #[test]
    fn crlf_and_lf_outputs_differ_only_in_line_terminators() {
        let g = busy_game(13);
        let lf = render(&g, &WriteOptions::default());
        let crlf = render(&g, &with(|o| o.newline = Newline::CrLf));
        assert_eq!(crlf, lf.replace('\n', "\r\n"));
    }

    #[test]
    fn column_style_text_is_stable_across_widths() {
        // Column rows do not wrap (one full move per row): only check that
        // the options do not break balance or lose the result.
        let g = busy_game(21);
        for w in [
            LineWidth::Unlimited,
            LineWidth::Columns(20),
            LineWidth::Columns(80),
        ] {
            let o = with(|o| {
                o.column = true;
                o.width = w;
            });
            let mt = movetext(&g, &o);
            paren_depth(&mt).unwrap();
            assert!(mt.trim_end().ends_with(g.tags().result.as_str()));
        }
    }
}

// ===========================================================================
// 4. Encoding
// ===========================================================================

mod encoding {
    use super::*;

    fn accented_game() -> Game {
        let mut g = sample_tree();
        g.tags_mut().white = "M\u{fc}ller \u{265f} \u{20ac}".into();
        g.set_initial_comment("Zo\u{eb} \u{65e5}\u{672c}");
        g
    }

    #[test]
    fn utf8_writes_characters_as_utf8_without_bom() {
        let bytes = bytes_of(&accented_game(), &WriteOptions::default());
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("[White \"M\u{fc}ller \u{265f} \u{20ac}\"]"));
        assert!(s.contains("{Zo\u{eb} \u{65e5}\u{672c}}"));
    }

    #[test]
    fn utf8_bom_through_pgn_writer_starts_with_ef_bb_bf_followed_by_the_utf8_text() {
        let o = with(|o| o.encoding = Encoding::Utf8Bom);
        let mut w = PgnWriter::new(Vec::new(), o);
        w.write_game(&accented_game()).unwrap();
        let bytes = w.finish().unwrap();
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        let plain = bytes_of(&accented_game(), &WriteOptions::default());
        assert_eq!(&bytes[3..], &plain[..]);
        assert!(!bytes[3..].starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn write_game_never_emits_a_bom() {
        for enc in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Latin1] {
            let o = with(|o| o.encoding = enc);
            let bytes = bytes_of(&accented_game(), &o);
            assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "{enc:?}");
        }
        let bom = bytes_of(&accented_game(), &with(|o| o.encoding = Encoding::Utf8Bom));
        assert_eq!(bom, bytes_of(&accented_game(), &WriteOptions::default()));
    }

    #[test]
    fn latin1_writes_one_byte_per_char_and_question_marks_for_the_rest() {
        let o = with(|o| o.encoding = Encoding::Latin1);
        let bytes = bytes_of(&accented_game(), &o);
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        // "Müller ♟ €" -> M FC l l e r SP ? SP ?
        let needle = b"[White \"M\xFCller ? ?\"]";
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "{:?}",
            latin1_to_string(&bytes)
        );
        // "Zoë 日本" -> Zo EB SP ? ?
        let needle = b"{Zo\xEB ??}";
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "{:?}",
            latin1_to_string(&bytes)
        );
    }

    #[test]
    fn latin1_replaces_each_character_not_each_byte_or_grapheme() {
        let mut g = Game::new();
        // astral emoji (one char, 4 UTF-8 bytes) and "e" + combining acute (two chars)
        g.tags_mut().white = "\u{1f600}|e\u{301}|\u{100}|\u{ff}|\u{a0}".into();
        let o = with(|o| o.encoding = Encoding::Latin1);
        let bytes = bytes_of(&g, &o);
        let needle = b"[White \"?|e?|?|\xFF|\xA0\"]";
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "{:?}",
            latin1_to_string(&bytes)
        );
    }

    #[test]
    fn latin1_output_is_valid_latin1_for_every_byte_and_ascii_structure() {
        let o = with(|o| o.encoding = Encoding::Latin1);
        let g = busy_game(17);
        let bytes = bytes_of(&g, &o);
        let utf8 = render(&g, &o);
        let expected: Vec<u8> = utf8
            .chars()
            .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
            .collect();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn latin1_wrapping_counts_characters() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, &"\u{e9}\u{e9}\u{e9} ".repeat(40))
            .unwrap();
        let o = with(|o| {
            o.encoding = Encoding::Latin1;
            o.width = LineWidth::Columns(20);
        });
        let bytes = bytes_of(&g, &o);
        let s = latin1_to_string(&bytes);
        let mt = s.split_once("\n\n").unwrap().1;
        check_wrapping(mt, Some(20), true).unwrap();
        assert!(bytes.iter().all(|&b| b != 0xC3), "UTF-8 lead byte leaked");
    }

    #[test]
    fn game_to_string_is_always_utf8_without_bom() {
        // Observation: the encoding option only applies when bytes are written.
        for enc in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Latin1] {
            let o = with(|o| o.encoding = enc);
            let s = render(&accented_game(), &o);
            assert!(s.starts_with("[Event"), "{enc:?}");
            assert!(s.contains("M\u{fc}ller \u{265f} \u{20ac}"), "{enc:?}");
        }
    }

    #[test]
    fn crlf_and_bom_combine() {
        let o = with(|o| {
            o.encoding = Encoding::Utf8Bom;
            o.newline = Newline::CrLf;
        });
        let mut w = PgnWriter::new(Vec::new(), o);
        w.write_game(&sample_tree()).unwrap();
        let bytes = w.finish().unwrap();
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        assert!(bytes.windows(2).any(|w| w == b"\r\n"));
    }
}

// ===========================================================================
// 5. PgnWriter
// ===========================================================================

mod stream {
    use super::*;

    #[test]
    fn empty_stream_writes_nothing() {
        let w = PgnWriter::new(Vec::new(), WriteOptions::default());
        assert_eq!(w.games_written(), 0);
        assert_eq!(w.finish().unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn games_are_separated_by_exactly_one_blank_line_and_none_follows_the_last() {
        let a = sample_tree();
        let mut b = Game::new();
        b.tags_mut().white = "B".into();
        let mut c = Game::new();
        c.set_initial_comment("c");
        let mut w = PgnWriter::new(Vec::new(), WriteOptions::default());
        for (i, g) in [&a, &b, &c].into_iter().enumerate() {
            assert_eq!(w.games_written(), i as u64);
            w.write_game(g).unwrap();
        }
        assert_eq!(w.games_written(), 3);
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        let o = WriteOptions::default();
        let expected = format!("{}\n{}\n{}", render(&a, &o), render(&b, &o), render(&c, &o));
        assert_eq!(out, expected);
        assert!(out.ends_with("*\n") && !out.ends_with("\n\n"));
        assert_eq!(out.matches("\n\n[Event").count(), 2);
        assert!(!out.contains("\n\n\n"));
    }

    #[test]
    fn crlf_separator_between_games() {
        let o = with(|o| o.newline = Newline::CrLf);
        let mut w = PgnWriter::new(Vec::new(), o.clone());
        w.write_game(&sample_tree()).unwrap();
        w.write_game(&sample_tree()).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        let one = render(&sample_tree(), &o);
        assert_eq!(out, format!("{one}\r\n{one}"));
    }

    #[test]
    fn bom_appears_once_at_the_very_start_of_a_multi_game_stream() {
        let o = with(|o| o.encoding = Encoding::Utf8Bom);
        let mut w = PgnWriter::new(Vec::new(), o);
        for _ in 0..3 {
            w.write_game(&sample_tree()).unwrap();
        }
        let out = w.finish().unwrap();
        assert_eq!(&out[..3], &[0xEF, 0xBB, 0xBF]);
        assert_eq!(
            out.windows(3).filter(|w| *w == [0xEF, 0xBB, 0xBF]).count(),
            1
        );
    }

    #[test]
    fn latin1_stream_has_no_bom_and_encodes_every_game() {
        let o = with(|o| o.encoding = Encoding::Latin1);
        let mut g = sample_tree();
        g.tags_mut().black = "\u{e9}".into();
        let mut w = PgnWriter::new(Vec::new(), o);
        w.write_game(&g).unwrap();
        w.write_game(&g).unwrap();
        let out = w.finish().unwrap();
        assert_eq!(out.iter().copied().filter(|b| *b == 0xE9).count(), 2);
    }

    #[test]
    fn single_game_stream_equals_write_game() {
        let g = busy_game(2);
        for o in [WriteOptions::default(), WriteOptions::scid_export()] {
            let mut w = PgnWriter::new(Vec::new(), o.clone());
            w.write_game(&g).unwrap();
            assert_eq!(w.finish().unwrap(), bytes_of(&g, &o));
        }
    }

    #[test]
    fn short_writes_from_the_sink_still_produce_complete_output() {
        struct Dribble(Vec<u8>);
        impl Write for Dribble {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                let n = buf.len().min(3);
                self.0.extend_from_slice(&buf[..n]);
                Ok(n)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let g = busy_game(4);
        let o = WriteOptions::default();
        let mut sink = Dribble(Vec::new());
        write_game(&mut sink, &g, &o).unwrap();
        assert_eq!(sink.0, bytes_of(&g, &o));
    }

    #[test]
    fn io_errors_are_reported_not_swallowed() {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("disk full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("disk full"))
            }
        }
        assert!(write_game(&mut Full, &sample_tree(), &WriteOptions::default()).is_err());
        let mut w = PgnWriter::new(Full, WriteOptions::default());
        assert!(w.write_game(&sample_tree()).is_err());
    }

    #[test]
    fn pgn_writer_is_poisoned_after_an_io_error() {
        struct FailOnce(bool);
        impl Write for FailOnce {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                if self.0 {
                    Ok(buf.len())
                } else {
                    self.0 = true;
                    Err(io::Error::other("boom"))
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut w = PgnWriter::new(FailOnce(false), WriteOptions::default());
        assert!(matches!(
            w.write_game(&sample_tree()),
            Err(mychess_pgn::write::WriteError::Io(_))
        ));
        // The sink would now accept bytes, but the stream is poisoned.
        assert!(matches!(
            w.write_game(&sample_tree()),
            Err(mychess_pgn::write::WriteError::Io(_))
        ));
        assert!(matches!(
            w.finish(),
            Err(mychess_pgn::write::WriteError::Io(_))
        ));
    }

    #[test]
    fn writer_works_on_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pgn");
        let file = std::fs::File::create(&path).unwrap();
        let mut w = PgnWriter::new(io::BufWriter::new(file), WriteOptions::default());
        w.write_game(&sample_tree()).unwrap();
        w.write_game(&sample_tree()).unwrap();
        w.finish().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[Event").count(), 2);
    }
}

// ===========================================================================
// 6. Scale
// ===========================================================================

mod scale {
    use super::*;

    fn shuffle_game(plies: usize) -> Game {
        let mut g = Game::new();
        let mut cur = g.root();
        let cycle = ["Nf3", "Nf6", "Ng1", "Ng8"];
        for i in 0..plies {
            cur = play(&mut g, cur, cycle[i % 4]);
        }
        g
    }

    /// `depth` nested variations: at each level two sibling moves, the second
    /// one (the variation) continues.
    fn nested_game(depth: usize) -> (Game, usize) {
        let mut g = Game::new();
        let mut cur = g.root();
        let mut reached = 0;
        for _ in 0..depth {
            let moves = g.position_at(cur).unwrap().legal_moves();
            if moves.len() < 2 {
                break;
            }
            g.add_move(cur, moves[0]).unwrap();
            cur = g.add_move(cur, moves[1]).unwrap();
            reached += 1;
        }
        (g, reached)
    }

    #[test]
    fn thirty_thousand_plies_are_written_without_overflowing_the_stack() {
        let g = shuffle_game(30_000);
        let started = Instant::now();
        let mt = movetext(&g, &WriteOptions::default());
        assert!(
            started.elapsed().as_secs() < 30,
            "too slow: {:?}",
            started.elapsed()
        );
        check_wrapping(&mt, Some(80), true).unwrap();
        assert!(mt.ends_with(" *\n"));
        assert!(mt.contains("15000. Ng1"), "last white move number missing");
        assert_eq!(
            mt.split_whitespace().filter(|w| w.ends_with('.')).count(),
            15_000
        );
    }

    #[test]
    fn deep_variation_nesting_is_written_iteratively() {
        let (g, depth) = nested_game(2_500);
        assert!(depth >= 2_000, "test setup reached only depth {depth}");
        let started = Instant::now();
        let mt = movetext(&g, &with(|o| o.width = LineWidth::Unlimited));
        assert!(
            started.elapsed().as_secs() < 30,
            "too slow: {:?}",
            started.elapsed()
        );
        assert_eq!(paren_depth(&mt), Ok(depth));
    }

    #[test]
    fn deep_variation_nesting_with_indentation_and_wrapping() {
        let (g, depth) = nested_game(2_100);
        assert!(depth >= 2_000, "test setup reached only depth {depth}");
        let o = with(|o| o.indent_variations = true);
        let started = Instant::now();
        let mt = movetext(&g, &o);
        assert!(
            started.elapsed().as_secs() < 60,
            "too slow: {:?}",
            started.elapsed()
        );
        assert_eq!(paren_depth(&mt), Ok(depth));
        // Indentation grows by four spaces per level.
        let deepest = mt
            .lines()
            .map(|l| l.len() - l.trim_start().len())
            .max()
            .unwrap();
        assert!(deepest >= 4 * (depth - 1), "max indent {deepest}");
    }

    #[test]
    fn deep_nesting_with_a_long_main_line_and_stripped_variations() {
        let (g, _) = nested_game(2_100);
        let o = with(|o| o.variations = false);
        let mt = movetext(&g, &o);
        assert!(!mt.contains('('));
    }

    #[test]
    fn many_siblings_at_one_node() {
        let mut g = Game::new();
        let root = g.root();
        let moves = g.position_at(root).unwrap().legal_moves();
        for m in &moves {
            g.add_move(root, *m).unwrap();
        }
        let mt = mt_default(&g);
        assert_eq!(mt.matches('(').count(), moves.len() - 1);
        paren_depth(&mt).unwrap();
    }

    #[test]
    fn huge_comment_is_written_whole() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        let words = "word ".repeat(200_000);
        g.set_comment(e4, words.trim_end()).unwrap();
        let mt = mt_default(&g);
        check_wrapping(&mt, Some(80), true).unwrap();
        assert_eq!(mt.matches("word").count(), 200_000);
    }
}

// ===========================================================================
// 7. Properties
// ===========================================================================

fn width_from(i: u8) -> LineWidth {
    match i % 8 {
        0 => LineWidth::Unlimited,
        1 => LineWidth::Columns(1),
        2 => LineWidth::Columns(10),
        3 => LineWidth::Columns(20),
        4 => LineWidth::Columns(40),
        5 => LineWidth::Columns(75),
        6 => LineWidth::Columns(80),
        _ => LineWidth::Columns(200),
    }
}

fn options_from(bits: u32, width: u8, enc: u8) -> WriteOptions {
    with(|o| {
        o.comments = bits & 1 != 0;
        o.variations = bits & 2 != 0;
        o.indent_comments = bits & 4 != 0;
        o.indent_variations = bits & 8 != 0;
        o.move_number_space = bits & 16 != 0;
        o.column = bits & 32 != 0;
        o.strip_marks = bits & 64 != 0;
        o.null_moves = if bits & 128 != 0 {
            NullMoves::ConvertToComment
        } else {
            NullMoves::Keep
        };
        o.braces = if bits & 256 != 0 {
            BraceHandling::Strip
        } else {
            BraceHandling::ReplaceWithParens
        };
        o.nags = match (bits >> 9) & 3 {
            0 => NagOutput::None,
            1 => NagOutput::Symbolic,
            _ => NagOutput::Numeric,
        };
        o.tags = if bits & (1 << 11) != 0 {
            TagSet::SevenTagRoster
        } else {
            TagSet::All
        };
        o.newline = if bits & (1 << 12) != 0 {
            Newline::CrLf
        } else {
            Newline::Lf
        };
        o.scid_flags = bits & (1 << 13) != 0;
        o.header = if bits & (1 << 14) != 0 {
            Header::Short
        } else {
            Header::Pgn
        };
        o.width = width_from(width);
        o.encoding = match enc % 3 {
            0 => Encoding::Utf8,
            1 => Encoding::Utf8Bom,
            _ => Encoding::Latin1,
        };
    })
}

const START_FENS: &[Option<&str>] = &[
    None,
    Some(BLACK_TO_MOVE_FEN),
    Some("r3k2r/pppq1ppp/2npbn2/2b1p3/2B1P3/2NPBN2/PPPQ1PPP/R3K2R w KQkq - 4 40"),
    Some("4k3/8/8/8/8/8/4P3/4K3 b - - 0 99"),
];

fn decorate_tags(g: &mut Game, pick: u8, newlines_ok: bool) {
    let events = [
        "Open",
        "a \"quoted\" ev\\ent",
        "\u{dc}n\u{ef} \u{65e5}\u{672c}",
        "ends\\",
        "x] y",
    ];
    let t = g.tags_mut();
    events[usize::from(pick) % events.len()].clone_into(&mut t.event);
    if newlines_ok && pick.is_multiple_of(3) {
        t.event.push_str("\r\nsecond\nthird");
    }
    t.white = "White, W".into();
    t.black = "\u{c9}mile".into();
    t.result = match pick % 4 {
        0 => GameResult::Unknown,
        1 => GameResult::White,
        2 => GameResult::Black,
        _ => GameResult::Draw,
    };
    if pick.is_multiple_of(2) {
        t.white_rating = Some(Rating::new(
            Elo::new(2000 + u16::from(pick)).unwrap(),
            RatingType::Elo,
        ));
    }
    t.set_extra("ScidFlags", "UW").unwrap();
    t.set_extra("Extra", "v\"al").unwrap();
}

/// Everything the writer promises for one game and one option set.
fn check_game(g: &Game, o: &WriteOptions) -> Result<(), String> {
    let text = game_to_string(g, o).map_err(|e| format!("write error {e:?}"))?;

    // (a) bytes are valid for the encoding and match the text.
    let mut bytes = Vec::new();
    write_game(&mut bytes, g, o).map_err(|e| format!("write_game error {e:?}"))?;
    match o.encoding {
        Encoding::Utf8 => {
            if bytes != text.as_bytes() {
                return Err("utf-8 bytes differ from the text".into());
            }
        }
        Encoding::Utf8Bom => {
            // write_game never emits the BOM; only PgnWriter does.
            if bytes != text.as_bytes() {
                return Err("write_game output with Utf8Bom differs from the text".into());
            }
            let mut w = PgnWriter::new(Vec::new(), o.clone());
            w.write_game(g).map_err(|e| format!("{e:?}"))?;
            let streamed = w.finish().map_err(|e| format!("{e:?}"))?;
            if streamed.len() < 3
                || streamed[..3] != [0xEF, 0xBB, 0xBF]
                || streamed[3..] != *text.as_bytes()
            {
                return Err("PgnWriter BOM output differs".into());
            }
        }
        Encoding::Latin1 => {
            let expected: Vec<u8> = text
                .chars()
                .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
                .collect();
            if bytes != expected {
                return Err("latin-1 bytes differ".into());
            }
        }
        _ => {}
    }

    // newline discipline
    let crlf = o.newline == Newline::CrLf;
    let tb = text.as_bytes();
    for (i, &b) in tb.iter().enumerate() {
        if b == b'\n' && (crlf != (i > 0 && tb[i - 1] == b'\r')) {
            return Err(format!("wrong line terminator at byte {i}"));
        }
        if b == b'\r' && tb.get(i + 1) != Some(&b'\n') {
            return Err(format!("bare CR at byte {i}"));
        }
    }
    if !text.ends_with(if crlf { "\r\n" } else { "\n" }) {
        return Err("text does not end with a line terminator".into());
    }

    // header / movetext split
    let sep = if crlf { "\r\n\r\n" } else { "\n\n" };
    let (head, mt) = text
        .split_once(sep)
        .ok_or("no blank line after the header")?;
    if o.header == Header::Pgn {
        let pairs: Vec<(String, String)> = head
            .lines()
            .map(|l| parse_tag_line(l).ok_or(format!("bad tag line {l:?}")))
            .collect::<Result<_, _>>()?;
        let names: Vec<&str> = pairs.iter().map(|(n, _)| n.as_str()).take(7).collect();
        if names != ["Event", "Site", "Date", "Round", "White", "Black", "Result"] {
            return Err(format!("seven tag roster out of order: {names:?}"));
        }
        if norm_ws(&pairs[0].1) != norm_ws(&g.tags().event) {
            return Err(format!("Event value changed: {:?}", pairs[0].1));
        }
        if o.tags == TagSet::SevenTagRoster
            && pairs.len() != 7 + 2 * usize::from(g.start_position() != &Position::new())
        {
            return Err(format!(
                "unexpected tags with the seven tag roster: {pairs:?}"
            ));
        }
        if pairs.iter().any(|(n, _)| n == "ScidFlags") && !(o.scid_flags && o.tags == TagSet::All) {
            return Err("ScidFlags written without the option".into());
        }
    }

    // (b) wrapping, (c) balance
    let width = if o.column {
        None
    } else {
        Some(o.wrap_width_for_test())
    };
    check_wrapping(mt, width, true)?;
    paren_depth(mt)?;

    // (d) every move is legal SAN in the right position (not for column rows).
    if !o.column {
        let toks = tokenize(mt)?;
        let converting = o.null_moves == NullMoves::ConvertToComment && has_null(g);
        if converting {
            // Nothing outside a comment may be a null move.
            if toks.iter().any(|t| matches!(t, Tok::Word(w) if w == "--"))
                && !has_null_outside_comment_ok(mt)
            {
                return Err("null move written outside a comment".into());
            }
        } else {
            let rebuilt = walk(g, o, &toks)?;
            let expected = if o.variations {
                shape(g)
            } else {
                let mut main = g.clone();
                main.strip_variations();
                shape(&main)
            };
            if shape(&rebuilt) != expected {
                return Err("the written moves do not rebuild the original tree".into());
            }
        }
    }
    Ok(())
}

fn has_null_outside_comment_ok(_mt: &str) -> bool {
    false
}

trait WidthForTest {
    fn wrap_width_for_test(&self) -> usize;
}

impl WidthForTest for WriteOptions {
    fn wrap_width_for_test(&self) -> usize {
        match self.width {
            LineWidth::Columns(n) => usize::from(n.max(1)),
            _ => usize::MAX,
        }
    }
}

/// Deterministic by default (fixed seed, 96 cases). For a wider sweep set
/// `PGN_WRITE_PROPTEST_CASES` and `PGN_WRITE_PROPTEST_SEED`.
fn prop_config() -> ProptestConfig {
    let env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok());
    ProptestConfig {
        cases: env("PGN_WRITE_PROPTEST_CASES").map_or(96, |c| c as u32),
        rng_seed: proptest::test_runner::RngSeed::Fixed(
            env("PGN_WRITE_PROPTEST_SEED").unwrap_or(0x5eed_0f9a),
        ),
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/pgn_write.txt",
            ),
        )),
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(prop_config())]

    #[test]
    fn written_games_are_balanced_wrapped_encoded_and_legal(
        ops in prop::collection::vec((any::<u8>(), any::<u16>(), any::<u16>()), 0..90),
        fen in 0..START_FENS.len(),
        bits in any::<u32>(),
        width in any::<u8>(),
        enc in any::<u8>(),
        pick in any::<u8>(),
    ) {
        let mut g = build(&ops, START_FENS[fen]);
        let o = options_from(bits, width, enc);
        decorate_tags(&mut g, pick, o.header == Header::Pgn);
        let r = check_game(&g, &o);
        prop_assert!(r.is_ok(), "{:?}\noptions: {:?}\n{}", r, o, game_to_string(&g, &o).unwrap_or_default());
    }

    #[test]
    fn strict_defaults_always_rebuild_the_same_tree(
        ops in prop::collection::vec((any::<u8>(), any::<u16>(), any::<u16>()), 0..120),
        fen in 0..START_FENS.len(),
    ) {
        let g = build(&ops, START_FENS[fen]);
        let o = WriteOptions::default();
        let r = check_game(&g, &o);
        prop_assert!(r.is_ok(), "{:?}\n{}", r, game_to_string(&g, &o).unwrap_or_default());
    }

    #[test]
    fn symbolic_nags_never_break_the_walk(
        ops in prop::collection::vec((any::<u8>(), any::<u16>(), any::<u16>()), 0..80),
    ) {
        let g = build(&ops, None);
        let o = with(|o| o.nags = NagOutput::Symbolic);
        let r = check_game(&g, &o);
        prop_assert!(r.is_ok(), "{:?}\n{}", r, game_to_string(&g, &o).unwrap_or_default());
    }

    #[test]
    fn writing_is_deterministic_and_streaming_matches_single_games(
        ops in prop::collection::vec((any::<u8>(), any::<u16>(), any::<u16>()), 0..60),
        bits in any::<u32>(),
        enc in any::<u8>(),
    ) {
        let g = build(&ops, None);
        let o = options_from(bits, 6, enc);
        let one = bytes_of(&g, &o);
        prop_assert_eq!(&one, &bytes_of(&g, &o));
        let mut w = PgnWriter::new(Vec::new(), o.clone());
        w.write_game(&g).unwrap();
        w.write_game(&g).unwrap();
        let two = w.finish().unwrap();
        let bom: &[u8] = if o.encoding == Encoding::Utf8Bom { &[0xEF, 0xBB, 0xBF] } else { &[] };
        prop_assert!(!one.starts_with(&[0xEF, 0xBB, 0xBF]));
        prop_assert!(two.starts_with(bom));
        let two = &two[bom.len()..];
        prop_assert!(two.starts_with(&one));
        let sep: &[u8] = if o.newline == Newline::CrLf { b"\r\n" } else { b"\n" };
        let rest = &two[one.len()..];
        prop_assert!(rest.starts_with(sep));
        let second = &rest[sep.len()..];
        prop_assert_eq!(second, &one[..]);
    }
}

// ===========================================================================
// 8. Snapshots
// ===========================================================================

mod snapshots {
    use super::*;

    /// Morphy's opera game with variations, NAGs, comments and tags.
    fn opera_game() -> Game {
        let mut g = Game::new();
        let t = g.tags_mut();
        t.event = "Paris Opera".into();
        t.site = "Paris FRA".into();
        t.date = PartialDate::new(Some(1858), None, None).unwrap();
        t.round = "?".into();
        t.white = "Morphy, Paul".into();
        t.black = "Duke of Brunswick and Count Isouard".into();
        t.result = GameResult::White;
        t.eco = Some("C41".parse::<Eco>().unwrap());
        t.white_rating = Some(Rating::new(Elo::new(2690).unwrap(), RatingType::Elo));
        t.set_extra("Annotator", "Test \"suite\"").unwrap();
        t.set_extra("ScidFlags", "UW").unwrap();
        let root = g.root();
        g.set_initial_comment("The famous Opera Box game, played at a performance of Norma.");
        let e4 = play(&mut g, root, "e4");
        let e5 = play(&mut g, e4, "e5");
        let nf3 = play(&mut g, e5, "Nf3");
        let d6 = play(&mut g, nf3, "d6");
        g.add_nag(d6, nag(6)).unwrap();
        g.set_comment(d6, "The Philidor Defence. [%clk 0:58:00]")
            .unwrap();
        // alternative: 2...Nc6
        let nc6 = play(&mut g, nf3, "Nc6");
        g.set_comment_before(nc6, "more usual").unwrap();
        let bb5 = play(&mut g, nc6, "Bb5");
        g.add_nag(bb5, nag(14)).unwrap();
        let d4 = play(&mut g, d6, "d4");
        let bg4 = play(&mut g, d4, "Bg4");
        // alternative: 3...Nf6 with a nested one
        let nf6 = play(&mut g, d4, "Nf6");
        let nc3 = play(&mut g, nf6, "Nc3");
        play(&mut g, nc3, "Be7");
        let dxe5 = play(&mut g, bg4, "dxe5");
        let bxf3 = play(&mut g, dxe5, "Bxf3");
        g.add_nag(bxf3, nag(2)).unwrap();
        let qxf3 = play(&mut g, bxf3, "Qxf3");
        let dxe5b = play(&mut g, qxf3, "dxe5");
        let bc4 = play(&mut g, dxe5b, "Bc4");
        g.add_nag(bc4, nag(1)).unwrap();
        g.set_comment(bc4, "Threatening mate on f7 {see below}")
            .unwrap();
        let nf6b = play(&mut g, bc4, "Nf6");
        let qb3 = play(&mut g, nf6b, "Qb3");
        g.add_nag(qb3, nag(3)).unwrap();
        g.add_nag(qb3, nag(7)).unwrap();
        let qe7 = play(&mut g, qb3, "Qe7");
        let nc3b = play(&mut g, qe7, "Nc3");
        let c6 = play(&mut g, nc3b, "c6");
        let bg5 = play(&mut g, c6, "Bg5");
        let b5 = play(&mut g, bg5, "b5");
        g.set_comment(
            b5,
            "Black has no good defence anyway; a very long comment follows to exercise the \
            line wrapping of the writer at the configured width and keep going for a while.",
        )
        .unwrap();
        let nxb5 = play(&mut g, b5, "Nxb5");
        let cxb5 = play(&mut g, nxb5, "cxb5");
        let bxb5 = play(&mut g, cxb5, "Bxb5+");
        let nbd7 = play(&mut g, bxb5, "Nbd7");
        let oooo = play(&mut g, nbd7, "O-O-O");
        let rd8 = play(&mut g, oooo, "Rd8");
        let rxd7 = play(&mut g, rd8, "Rxd7");
        let rxd7b = play(&mut g, rxd7, "Rxd7");
        let rd1 = play(&mut g, rxd7b, "Rd1");
        let qe6 = play(&mut g, rd1, "Qe6");
        let bxd7 = play(&mut g, qe6, "Bxd7+");
        let nxd7 = play(&mut g, bxd7, "Nxd7");
        let qb8 = play(&mut g, nxd7, "Qb8+");
        g.add_nag(qb8, nag(3)).unwrap();
        let nxb8 = play(&mut g, qb8, "Nxb8");
        play(&mut g, nxb8, "Rd8#");
        g
    }

    #[test]
    fn opera_game_with_strict_defaults() {
        insta::assert_snapshot!(
            "opera_default",
            render(&opera_game(), &WriteOptions::default())
        );
    }

    #[test]
    fn opera_game_with_the_scid_export_preset_as_latin1_bytes() {
        let mut g = opera_game();
        // A null move inside a variation exercises ConvertToComment, and a
        // non-Latin-1 character exercises the `?` replacement.
        let root = g.root();
        let e4 = g.children(root).unwrap()[0];
        let c5 = play(&mut g, e4, "c5");
        g.set_comment(c5, "Sicilian \u{2013} not played by M\u{f8}rphy")
            .unwrap();
        let nf3 = play(&mut g, c5, "Nf3");
        let null = g.add_move(nf3, Move::Null).unwrap();
        let d4 = play(&mut g, null, "d4");
        play(&mut g, d4, "cxd4");
        let bytes = bytes_of(&g, &WriteOptions::scid_export());
        insta::assert_snapshot!("opera_scid_export_latin1", latin1_to_string(&bytes));
    }

    #[test]
    fn fen_start_black_to_move_symbolic_nags_no_move_number_space_narrow_width() {
        let mut g = fen_game(BLACK_TO_MOVE_FEN);
        let t = g.tags_mut();
        t.event = "Endgame \\ study".into();
        t.white = "A".into();
        t.black = "B".into();
        t.result = GameResult::Draw;
        t.black_rating = Some(Rating::new(Elo::new(2100).unwrap(), RatingType::Dwz));
        t.event_date = PartialDate::new(Some(2001), Some(5), Some(6)).unwrap();
        t.set_extra("ScidFlags", "WB").unwrap();
        let root = g.root();
        g.set_initial_comment("Start comment");
        let e5 = play(&mut g, root, "e5");
        g.add_nag(e5, nag(1)).unwrap();
        g.add_nag(e5, nag(14)).unwrap();
        let c5 = play(&mut g, root, "c5");
        g.add_nag(c5, nag(142)).unwrap();
        let nf3 = play(&mut g, e5, "Nf3");
        g.add_nag(nf3, nag(6)).unwrap();
        g.add_nag(nf3, nag(7)).unwrap();
        g.set_comment(nf3, "the usual").unwrap();
        let nc6 = play(&mut g, nf3, "Nc6");
        g.set_comment(nc6, "[%csl Gd4] kept").unwrap();
        play(&mut g, nc6, "Bb5");
        let o = with(|o| {
            o.nags = NagOutput::Symbolic;
            o.move_number_space = false;
            o.width = LineWidth::Columns(40);
            o.scid_flags = true;
        });
        insta::assert_snapshot!("fen_black_symbolic_glued_40", render(&g, &o));
    }

    #[test]
    fn column_style_with_the_short_header_and_indentation() {
        let mut g = short_header_game();
        let root = g.root();
        let e4 = g.children(root).unwrap()[0];
        let e5 = g.children(e4).unwrap()[0];
        let nf3 = play(&mut g, e5, "Nf3");
        g.set_comment(nf3, "Develops.").unwrap();
        play(&mut g, e4, "c5");
        let o = with(|o| {
            o.column = true;
            o.header = Header::Short;
        });
        insta::assert_snapshot!("column_short_header", render(&g, &o));
    }

    fn short_header_game() -> Game {
        let mut g = Game::new();
        let t = g.tags_mut();
        t.event = "Club".into();
        t.round = "3".into();
        t.site = "Oslo".into();
        t.date = PartialDate::new(Some(2020), Some(2), None).unwrap();
        t.white = "Carlsen, M".into();
        t.black = "Nakamura, H".into();
        t.result = GameResult::Draw;
        t.white_rating = Some(Rating::new(Elo::new(2850).unwrap(), RatingType::Elo));
        let root = g.root();
        line(&mut g, root, "e4 e5 Nc3 Nc6");
        g
    }
}

// ===========================================================================
// 9. Observations (pinned behaviours the implementer flagged)
// ===========================================================================

mod observations {
    use super::*;

    #[test]
    fn scid_divergence_d32_black_numbers_and_parentheses_have_no_inner_spaces() {
        // D32(a)(b): Scid writes `1. ...e5` and `( 1... c5 2. Nf3 )`.
        let mt = mt_default(&sample_tree());
        assert!(mt.contains("e5 (1... c5 2. Nf3) 2. Nf3"), "{mt:?}");
        assert!(
            !mt.contains("( ") && !mt.contains(" )") && !mt.contains(". ..."),
            "{mt:?}"
        );
    }

    #[test]
    fn scid_divergence_d32_nags_survive_when_comments_are_off() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.add_nag(e4, nag(3)).unwrap();
        let o = with(|o| o.comments = false);
        assert!(movetext(&g, &o).contains("$3"));
    }

    #[test]
    fn game_to_string_returns_a_result() {
        let r: Result<String, mychess_pgn::write::WriteError> =
            game_to_string(&Game::new(), &WriteOptions::default());
        assert!(r.is_ok());
    }

    #[test]
    fn literal_open_brace_cannot_be_written_in_either_brace_mode() {
        for braces in [BraceHandling::ReplaceWithParens, BraceHandling::Strip] {
            let mut g = Game::new();
            let root = g.root();
            let e4 = play(&mut g, root, "e4");
            g.set_comment(e4, "{").unwrap();
            let o = with(|o| o.braces = braces);
            let mt = movetext(&g, &o);
            assert!(
                mt.matches('{').count() <= 1 && mt.matches('}').count() <= 1,
                "{mt:?}"
            );
        }
    }

    #[test]
    fn column_style_has_no_byte_parity_with_scid_for_comments_and_variations() {
        // D35: only the table rows are specified; comments and variations
        // end the table. The text must still carry all the information.
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "note").unwrap();
        play(&mut g, root, "d4");
        let mt = movetext(&g, &with(|o| o.column = true));
        assert!(mt.contains("{note}"), "{mt:?}");
        assert!(mt.contains("(1. d4)"), "{mt:?}");
    }
}

// ===========================================================================
// 10. More SAN, tag-set and comment hygiene cases
// ===========================================================================

mod extra {
    use super::*;

    #[test]
    fn promotion_with_capture_check_castling_en_passant_and_disambiguation_are_san() {
        let mut g = fen_game("r3k2r/1P6/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let root = g.root();
        line(&mut g, root, "bxa8=Q+ Kd7 O-O");
        assert_eq!(mt_default(&g), "1. bxa8=Q+ Kd7 2. O-O *\n");

        let mut g = fen_game("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
        let root = g.root();
        play(&mut g, root, "exd6");
        assert_eq!(mt_default(&g), "1. exd6 *\n");

        let mut g = fen_game("4k3/8/8/8/8/8/R6R/4K3 w - - 0 1");
        let root = g.root();
        line(&mut g, root, "Rae2");
        assert_eq!(mt_default(&g), "1. Rae2+ *\n");
    }

    #[test]
    fn seven_tag_roster_keeps_the_variant_tag_but_not_other_extras() {
        // Spec table: SevenTagRoster = the seven + FEN/SetUp/Variant.
        let mut g = Game::new();
        g.tags_mut().set_extra("Variant", "Chess960").unwrap();
        g.tags_mut().set_extra("Other", "x").unwrap();
        let text = render(&g, &with(|o| o.tags = TagSet::SevenTagRoster));
        assert!(text.contains("[Variant \"Chess960\"]\n"), "{text}");
        assert!(!text.contains("[Other"), "{text}");
    }

    #[test]
    fn carriage_returns_in_comments_become_the_output_line_terminator() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "a\rb\r\nc").unwrap();
        assert_eq!(mt_default(&g), "1. e4 {a\nb\nc} *\n");
        let o = with(|o| o.newline = Newline::CrLf);
        assert_eq!(movetext(&g, &o), "1. e4 {a\r\nb\r\nc} *\r\n");
    }

    #[test]
    fn trailing_spaces_before_an_explicit_comment_newline_are_dropped() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "x \n y").unwrap();
        let mt = mt_default(&g);
        check_wrapping(&mt, None, true).unwrap();
        assert_eq!(norm_ws(&mt), "1. e4 {x y} *");
    }

    #[test]

    fn percent_word_glued_to_its_predecessor_still_respects_the_width() {
        // Spec rule 32 + "a word starting with % never starts a line": the
        // `%` word travels with the word before it, so `{foo %bar}` (10
        // characters) must move to its own line at width 12 instead of
        // overflowing "1. e4 {foo %bar}" (16 characters).
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "foo %bar").unwrap();
        let o = with(|o| o.width = LineWidth::Columns(12));
        let mt = movetext(&g, &o);
        assert!(
            mt.lines()
                .all(|l| l.chars().count() <= 12 && !l.starts_with('%')),
            "{mt}"
        );
    }

    #[test]

    fn percent_words_in_comments_never_make_a_splittable_line_overflow() {
        let mut g = Game::new();
        let root = g.root();
        let e4 = play(&mut g, root, "e4");
        g.set_comment(e4, "foo %bar baz %q %r qux").unwrap();
        line(&mut g, e4, "e5 Nf3 Nc6 Bb5 a6");
        for w in 10..=40u16 {
            let o = with(|o| o.width = LineWidth::Columns(w));
            let mt = movetext(&g, &o);
            check_wrapping(&mt, Some(usize::from(w)), true)
                .unwrap_or_else(|e| panic!("width {w}: {e}\n{mt}"));
        }
    }

    #[test]
    fn comment_before_on_a_main_line_node_is_not_representable_and_is_not_written() {
        // Observation: after `promote_variation` the old variation keeps its
        // `comment_before` on a main-line node; PGN has no place for it and
        // the writer drops it silently (spec rule 28 only covers variations).
        let mut g = Game::new();
        let root = g.root();
        play(&mut g, root, "e4");
        let d4 = play(&mut g, root, "d4");
        g.set_comment_before(d4, "pre").unwrap();
        g.promote_variation(d4).unwrap();
        assert_eq!(mt_default(&g), "1. d4 (1. e4) *\n");
    }
}

#[test]
fn large_random_trees_satisfy_every_output_invariant_for_several_option_sets() {
    let sets = [
        WriteOptions::default(),
        WriteOptions::scid_export(),
        options_from(0b0_0001_1111_1111, 4, 0),
        options_from(0b1_0101_0010_1011, 3, 2),
        with(|o| {
            o.nags = NagOutput::Symbolic;
            o.move_number_space = false;
            o.width = LineWidth::Columns(33);
            o.newline = Newline::CrLf;
        }),
    ];
    for seed in 0..40u64 {
        let g = build(
            &random_ops(seed * 7919 + 1, 700),
            START_FENS[(seed % 4) as usize],
        );
        for (i, o) in sets.iter().enumerate() {
            check_game(&g, o).unwrap_or_else(|e| {
                panic!(
                    "seed {seed}, option set {i}: {e}\n{}",
                    game_to_string(&g, o).unwrap()
                )
            });
        }
    }
}
