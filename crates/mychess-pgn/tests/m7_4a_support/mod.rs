//! Shared fixtures and checkers of the M7-4a suites (HTML and LaTeX writers).
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::expect_used)]

use mychess_core::Game;
use mychess_pgn::export::DiagramSource;
use mychess_pgn::read::{PgnReader, ReadOptions};

pub fn read_games(pgn: &str) -> Vec<Game> {
    PgnReader::new(pgn.as_bytes(), ReadOptions::default())
        .map(|r| r.expect("in-memory read").game)
        .collect()
}

/// Game 1: variation, comment with markup, NAG `!` and `D`, hostile names.
pub const GAME_ONE: &str = r#"[Event "A&B <Open> \"Q\" 'x'"]
[Site "Rio_de $Janeiro % #"]
[Date "2024.05.01"]
[Round "3"]
[White "Smith & <Co>"]
[Black "O'Neil \"Z\""]
[Result "1-0"]
[WhiteElo "2500"]
[ECO "B20"]

1. e4 $1 {Good <b>move</b> & [%cal Ge2e4] ~ ^ \ ok} 1... c5 $201 (1... e5 {a "var" </script> ]]>} 2. Nf3 (2. f4 $6) 2... Nc6) 2. Nf3 d6 1-0
"#;

/// Game 2: starts from a FEN, result `*`.
pub const GAME_TWO: &str = r#"[Event "Endgame"]
[Result "*"]
[SetUp "1"]
[FEN "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"]

1. e4 Kd7 2. e5 *
"#;

pub fn fixtures() -> Vec<Game> {
    let mut games = read_games(GAME_ONE);
    games.extend(read_games(GAME_TWO));
    games
}

/// Records the requests and answers with a tiny SVG that has a `<title>`.
#[derive(Default)]
pub struct FakeDiagrams {
    pub fens: Vec<String>,
}

impl DiagramSource for FakeDiagrams {
    fn svg(&mut self, fen: &str, flip: bool, px: u16) -> String {
        self.fens.push(fen.to_owned());
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{px}\" height=\"{px}\" data-flip=\"{flip}\"><title>{fen}</title><rect width=\"1\" height=\"1\"/></svg>"
        )
    }
}

/// Checks that the tags of `html` are balanced; the content of `<script>`
/// elements is skipped. Returns the error as text.
pub fn check_balanced(html: &str) -> Result<(), String> {
    const VOID: [&str; 6] = ["meta", "br", "hr", "img", "link", "input"];
    let mut stack: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find('<') {
        rest = &rest[pos..];
        if let Some(r) = rest.strip_prefix("<!--") {
            let end = r.find("-->").ok_or("unterminated comment")?;
            rest = &r[end + 3..];
            continue;
        }
        if rest.starts_with("<!") {
            let end = rest.find('>').ok_or("unterminated declaration")?;
            rest = &rest[end + 1..];
            continue;
        }
        let end = rest.find('>').ok_or("unterminated tag")?;
        let tag = &rest[1..end];
        if tag.contains('<') {
            return Err(format!("raw '<' inside a tag: {tag}"));
        }
        rest = &rest[end + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim().to_ascii_lowercase();
            match stack.pop() {
                Some(open) if open == name => {}
                other => return Err(format!("closing {name} but open is {other:?}")),
            }
            continue;
        }
        if tag.ends_with('/') {
            continue;
        }
        let name: String = tag
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() {
            return Err(format!("empty tag name in <{tag}>"));
        }
        if VOID.contains(&name.as_str()) {
            continue;
        }
        if name == "script" || name == "style" {
            let close = format!("</{name}>");
            let end = rest.find(&close).ok_or("unterminated script")?;
            rest = &rest[end + close.len()..];
            continue;
        }
        stack.push(name);
    }
    if stack.is_empty() {
        Ok(())
    } else {
        Err(format!("unclosed tags: {stack:?}"))
    }
}
