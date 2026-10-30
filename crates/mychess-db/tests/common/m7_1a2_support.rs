//! Helpers of the M7-1a2 tests: small SI4 databases written with `Si4Writer`
//! from `mychess_core::Game`s (fictitious names only).
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc
)]

use std::path::{Path, PathBuf};

use mychess_core::{Game, GameResult, Nag, NodeId, PartialDate};
use mychess_db::si4::{GameFlags, PreparedGame, RawGame, Si4Database, Si4Writer};

pub fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game.position_at(parent).unwrap().parse_uci(uci).unwrap();
    game.add_move(parent, mv).unwrap()
}

/// A game with only the main line `ucis`; Alfa x Beta, Aberto, Rio, 2020.
pub fn line(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut at = game.root();
    for u in ucis {
        at = play(&mut game, at, u);
    }
    let tags = game.tags_mut();
    tags.white = "Alfa, A".into();
    tags.black = "Beta, B".into();
    tags.event = "Aberto".into();
    tags.site = "Rio".into();
    tags.round = "1".into();
    tags.date = PartialDate::parse_lenient("2020.01.10");
    tags.result = GameResult::White;
    game
}

/// A game with no comment, variation or NAG.
pub fn plain() -> Game {
    line(&["e2e4", "e7e5", "g1f3", "b8c6"])
}

/// A game with 2 comments (one with an embedded command), 2 variations (one
/// nested in the other), 1 comment inside a variation and 2 NAGs.
pub fn rich() -> Game {
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let e5 = play(&mut game, e4, "e7e5");
    let nf3 = play(&mut game, e5, "g1f3");
    play(&mut game, nf3, "b8c6");
    game.set_comment(e4, "[%eval 0.3] the king pawn").unwrap();
    game.set_comment(nf3, "develops").unwrap();
    game.add_nag(e5, Nag::new(1).unwrap()).unwrap();
    game.add_nag(nf3, Nag::new(14).unwrap()).unwrap();
    // A variation at move 1...: 1...c5 2.Nf3, with an alternative 2.d4 inside.
    let c5 = play(&mut game, e4, "c7c5");
    play(&mut game, c5, "g1f3");
    play(&mut game, c5, "d2d4");
    game.set_comment(c5, "Sicilian").unwrap();
    let tags = game.tags_mut();
    tags.white = "Alfa, A".into();
    tags.black = "Beta, B".into();
    tags.event = "Aberto".into();
    tags.site = "Rio".into();
    tags.round = "1".into();
    tags.result = GameResult::Draw;
    game
}

/// Writes a database `a2` in `dir` with these games (no flags) and returns
/// its base path.
pub fn build_games(dir: &Path, games: &[Game]) -> PathBuf {
    let base = dir.join("a2");
    let mut writer = Si4Writer::create(&base, 1).unwrap();
    let prepared: Vec<PreparedGame> = games
        .iter()
        .map(|g| PreparedGame::from_game(g, GameFlags::empty()).unwrap())
        .collect();
    writer.add_prepared_batch(&prepared).unwrap();
    drop(writer);
    base
}

/// The three files `(.si4, .sn4, .sg4)`.
pub fn snapshot(base: &Path) -> [Vec<u8>; 3] {
    ["si4", "sn4", "sg4"].map(|e| std::fs::read(base.with_extension(e)).unwrap())
}

/// The 47 bytes of every entry of an index file.
pub fn entries_of(index: &[u8]) -> Vec<&[u8]> {
    index[182..]
        .as_chunks::<47>()
        .0
        .iter()
        .map(<[u8; 47]>::as_slice)
        .collect()
}

pub fn open(base: &Path) -> Si4Database<std::fs::File> {
    Si4Database::open(base).unwrap()
}

/// What a record holds, counted on its raw tree.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Shape {
    pub comments: usize,
    pub variations: usize,
    pub nags: usize,
    pub mainline_moves: usize,
}

pub fn shape(db: &mut Si4Database<std::fs::File>, n: u32) -> Shape {
    let raw: RawGame = db.raw_game(n).unwrap();
    let mut shape = Shape {
        comments: 0,
        variations: 0,
        nags: 0,
        mainline_moves: raw.mainline().len(),
    };
    // Count only the lines reachable from the main line.
    let mut stack = vec![0usize];
    while let Some(l) = stack.pop() {
        let line = &raw.lines[l];
        shape.comments += usize::from(line.start_comment.is_some());
        for node in &line.moves {
            shape.comments += usize::from(node.comment.is_some());
            shape.nags += node.nags.len();
            shape.variations += node.variations.len();
            stack.extend(node.variations.iter().copied());
        }
    }
    shape
}

pub fn record(db: &mut Si4Database<std::fs::File>, n: u32) -> Vec<u8> {
    db.record_bytes(n).unwrap().to_vec()
}
