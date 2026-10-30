//! Shared helpers of the PGN reader contract suites. Only the public API of
//! `mychess_pgn` and `mychess_core` is used.
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::expect_used)]

use mychess_core::{Game, NodeId, Position};
use mychess_pgn::read::{DiagnosticKind, ParsedGame, PgnReader, ReadOptions};

/// Reads every game of `bytes`; an `Err` from the reader fails the test.
pub fn read_bytes(bytes: &[u8], opts: &ReadOptions) -> Vec<ParsedGame> {
    PgnReader::new(bytes, opts.clone())
        .map(|r| r.expect("no I/O error on an in-memory slice"))
        .collect()
}

pub fn read_all(text: &str) -> Vec<ParsedGame> {
    read_bytes(text.as_bytes(), &ReadOptions::default())
}

/// Reads `text` and asserts that it holds exactly one game.
pub fn one(text: &str) -> ParsedGame {
    one_with(text, &ReadOptions::default())
}

pub fn one_with(text: &str, opts: &ReadOptions) -> ParsedGame {
    let mut games = read_bytes(text.as_bytes(), opts);
    assert_eq!(games.len(), 1, "expected exactly one game in {text:?}");
    games.remove(0)
}

/// A game with the given header lines and movetext.
pub fn game_text(tags: &str, moves: &str) -> String {
    format!("[Event \"T\"]\n{tags}\n{moves}\n")
}

/// Reads a game made of a header and movetext.
pub fn mt(moves: &str) -> ParsedGame {
    one(&format!("[Event \"T\"]\n\n{moves}\n"))
}

pub fn mt_with(moves: &str, opts: &ReadOptions) -> ParsedGame {
    one_with(&format!("[Event \"T\"]\n\n{moves}\n"), opts)
}

pub fn kinds(pg: &ParsedGame) -> Vec<DiagnosticKind> {
    pg.diagnostics.iter().map(|d| d.kind.clone()).collect()
}

/// Kinds with `Info` diagnostics left out.
pub fn problems(pg: &ParsedGame) -> Vec<DiagnosticKind> {
    pg.diagnostics
        .iter()
        .filter(|d| d.severity > mychess_pgn::read::Severity::Info)
        .map(|d| d.kind.clone())
        .collect()
}

pub fn has(pg: &ParsedGame, f: impl Fn(&DiagnosticKind) -> bool) -> bool {
    pg.diagnostics.iter().any(|d| f(&d.kind))
}

pub fn count(pg: &ParsedGame, f: impl Fn(&DiagnosticKind) -> bool) -> usize {
    pg.diagnostics.iter().filter(|d| f(&d.kind)).count()
}

fn node_text(g: &Game, n: NodeId, out: &mut Vec<String>) {
    let parent = g.parent(n).expect("node").expect("not root");
    let pos: &Position = g.position_at(parent).expect("position");
    let mv = g.mv(n).expect("move");
    if let Some(b) = g.comment_before(n).expect("comment_before") {
        out.push(format!("{{{b}}}"));
    }
    out.push(pos.san(mv).expect("san of a stored move"));
    for nag in g.nags(n).expect("nags") {
        out.push(format!("${}", nag.value()));
    }
    if let Some(c) = g.comment(n).expect("comment") {
        out.push(format!("{{{c}}}"));
    }
}

fn dump_from(g: &Game, parent: NodeId, out: &mut Vec<String>) {
    let ch = g.children(parent).expect("children").to_vec();
    let Some(&first) = ch.first() else { return };
    node_text(g, first, out);
    for &v in &ch[1..] {
        out.push("(".into());
        node_text(g, v, out);
        dump_from(g, v, out);
        out.push(")".into());
    }
    dump_from(g, first, out);
}

/// Canonical one-line text of the whole tree (moves in SAN, `$n`, `{comment}`,
/// variations in parentheses). Recursive: for shallow trees only.
pub fn dump(g: &Game) -> String {
    let mut out = Vec::new();
    if let Some(c) = g.initial_comment() {
        out.push(format!("{{{c}}}"));
    }
    dump_from(g, g.root(), &mut out);
    out.join(" ").replace("( ", "(").replace(" )", ")")
}

/// SAN of the main line.
pub fn main_sans(g: &Game) -> Vec<String> {
    let mut v = Vec::new();
    let mut parent = g.root();
    for n in g.mainline() {
        v.push(
            g.position_at(parent)
                .unwrap()
                .san(g.mv(n).unwrap())
                .unwrap(),
        );
        parent = n;
    }
    v
}

/// Maximum variation nesting depth, computed iteratively.
pub fn max_depth(g: &Game) -> usize {
    let mut best = 0;
    let mut stack = vec![(g.root(), 0usize)];
    while let Some((n, d)) = stack.pop() {
        best = best.max(d);
        for (i, &c) in g.children(n).unwrap().iter().enumerate() {
            stack.push((c, if i == 0 { d } else { d + 1 }));
        }
    }
    best
}

/// Deterministic xorshift for fixtures that must not depend on a crate.
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}
