//! Fixtures of the M7-4a suites: synthetic games, a six-game SI4 database
//! and small readers. Fictitious data only.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::format_push_string
)]

use std::fs::File;
use std::path::{Path, PathBuf};

use mychess_core::Game;
use mychess_db::ExportItem;
use mychess_db::si4::{GameFlags, Si4Database, Si4Writer};
use mychess_pgn::read::{PgnReader, ReadOptions};

pub fn games_from_pgn(pgn: &str) -> Vec<Game> {
    PgnReader::new(pgn.as_bytes(), ReadOptions::default())
        .map(|r| r.expect("in-memory read").game)
        .collect()
}

/// 100 moves of knight shuffling (200 half moves).
pub fn long_moves() -> String {
    let mut out = String::new();
    for (k, mv) in ["Nf3", "Nf6", "Ng1", "Ng8"]
        .iter()
        .cycle()
        .take(200)
        .enumerate()
    {
        if k % 2 == 0 {
            out.push_str(&format!("{}. ", k / 2 + 1));
        }
        out.push_str(mv);
        out.push(' ');
    }
    out
}

/// The six games of the database (see `six_game_db`).
pub fn six_pgn() -> String {
    let long = long_moves();
    format!(
        r#"[Event "Aberto"]
[Site "Rio BRA"]
[Date "2020.05.17"]
[Round "3"]
[White "Silva, João"]
[Black "Müller"]
[Result "1-0"]
[WhiteElo "2500"]
[ECO "B20"]

1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 5. Nc3 a6 1-0

[Event "Aberto"]
[Site "Rio BRA"]
[Date "2020.05.18"]
[Round "4"]
[White "Costa"]
[Black "Lima"]
[Result "1/2-1/2"]
[WhiteElo "2100"]
[BlackElo "2050"]

1. e4 c5 2. Nf3 e6 1/2-1/2

[Event "Problemas"]
[Site "?"]
[Result "*"]
[White "?"]
[Black "?"]
[SetUp "1"]
[FEN "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"]

1. e4 Kd7 2. e5 *

[Event "Longa"]
[Site "Paris FRA"]
[Date "2021.??.??"]
[Result "0-1"]
[White "Ana"]
[Black "Ünal Çelik"]
[BlackElo "1800"]

{long} 0-1

[Event "Aberto"]
[Site "Madrid ESP"]
[Date "2022.01.02"]
[Result "1-0"]
[White "Zed"]
[Black "Yan"]

1. d4 d5 2. c4 e6 1-0

[Event "Fim"]
[Site "Lisboa"]
[Date "2023.03.04"]
[Result "0-1"]
[White "Wu"]
[Black "Xi"]

1. f3 e5 2. g4 Qh4# 0-1
"#
    )
}

pub struct Db {
    pub dir: tempfile::TempDir,
    pub db: Si4Database<File>,
}

/// Writes `games` (with flags) to an SI4 database in a temporary directory.
pub fn build_db(games: &[(Game, GameFlags)]) -> Db {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    for (game, flags) in games {
        writer.add_game(game, *flags).unwrap();
    }
    drop(writer);
    let db = Si4Database::open(&base).unwrap();
    Db { dir, db }
}

pub fn six_game_db() -> Db {
    let games: Vec<(Game, GameFlags)> = games_from_pgn(&six_pgn())
        .into_iter()
        .map(|g| (g, GameFlags::empty()))
        .collect();
    assert_eq!(games.len(), 6);
    build_db(&games)
}

pub fn items(db: &mut Si4Database<File>, numbers: &[u32]) -> Vec<ExportItem> {
    numbers.iter().map(|n| ExportItem::stored(db, *n)).collect()
}

/// Files in `dir`, sorted by name.
pub fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

pub fn path_in(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
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
