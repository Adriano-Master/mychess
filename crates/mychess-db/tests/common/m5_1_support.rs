//! Helpers of the M5-1 contract tests (opening tree engine). Games are built
//! in code from UCI text and written with `Si4Writer` (through the M3-3a
//! support module, which the including test file must declare as `base`).
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::cast_possible_truncation
)]

use mychess_core::{Game, GameResult, Position};
use mychess_db::GameSource;
use mychess_db::si4::Si4Database;
use mychess_db::tree::{
    CachedTree, NoCache, TreeCache, TreeError, TreeOptions, TreeQuery, TreeResult, TreeRow,
    TreeState, search,
};
use mychess_db::{CancelToken, Filter, NoProgress};

use crate::base::{B, Fixture, build};

/// A game from space-separated UCI moves, played from the standard start.
pub fn g(ucis: &str) -> Game {
    let mut game = Game::new();
    let mut at = game.root();
    for u in ucis.split_whitespace() {
        at = crate::base::play(&mut game, at, u);
    }
    game
}

/// A game builder (names "W" and "B") from UCI moves.
pub fn gb(ucis: &str) -> B {
    let mut b = B::of(g(ucis));
    b.0.tags_mut().white = "W".into();
    b.0.tags_mut().black = "B".into();
    b
}

/// `n` games `f(0)..f(n-1)`.
pub fn repeat(n: usize, f: impl Fn(usize) -> B) -> Vec<B> {
    (0..n).map(f).collect()
}

/// Writes the games to a temporary SI4 database and opens it.
pub fn db_of(games: Vec<B>) -> (Fixture, Si4Database) {
    let list: Vec<_> = games.into_iter().map(B::done).collect();
    let fx = build(&list);
    let db = fx.open();
    (fx, db)
}

/// The position after the UCI moves from the standard start.
pub fn pos_after(ucis: &str) -> Position {
    let mut pos = Position::new();
    for u in ucis.split_whitespace() {
        let mv = pos.parse_uci(u).unwrap();
        pos.play(mv).unwrap();
    }
    pos
}

/// Searches with a fresh all-in main filter, no cache and no progress, at
/// ply 0.
pub fn run(
    db: &mut impl GameSource,
    pos: &Position,
    opts: &TreeOptions<'_>,
    state: &mut TreeState,
) -> Result<TreeResult, TreeError> {
    let filter = Filter::new(db.len());
    run_with(db, pos, 0, &filter, opts, state, &mut NoCache)
}

pub fn run_with(
    db: &mut impl GameSource,
    pos: &Position,
    ply: u32,
    filter: &Filter,
    opts: &TreeOptions<'_>,
    state: &mut TreeState,
    cache: &mut dyn TreeCache,
) -> Result<TreeResult, TreeError> {
    let query = TreeQuery {
        position: pos,
        ply,
        main_filter: filter,
    };
    search(
        db,
        &query,
        opts,
        state,
        cache,
        &CancelToken::new(),
        &mut NoProgress,
    )
}

/// Search of a fresh state with default options.
pub fn tree(db: &mut impl GameSource, pos: &Position) -> TreeResult {
    run(db, pos, &TreeOptions::default(), &mut TreeState::default()).unwrap()
}

pub fn row<'a>(r: &'a TreeResult, san: &str) -> &'a TreeRow {
    r.rows
        .iter()
        .find(|row| row.san == san)
        .unwrap_or_else(|| panic!("no row {san} in {:?}", sans(r)))
}

pub fn sans(r: &TreeResult) -> Vec<&str> {
    r.rows.iter().map(|row| row.san.as_str()).collect()
}

pub fn with_result(b: B, result: GameResult) -> B {
    b.result(result)
}

/// An in-memory `TreeCache` that counts its calls. Keyed by the first two
/// FEN fields (squares and side to move), as the search matches.
#[derive(Default)]
pub struct MemCache {
    entries: Vec<(String, CachedTree)>,
    pub lookups: u32,
    pub stores: u32,
    pub last_len: Option<u32>,
}

fn key(pos: &Position) -> String {
    pos.fen().split(' ').take(2).collect::<Vec<_>>().join(" ")
}

impl MemCache {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl TreeCache for MemCache {
    fn lookup(&mut self, pos: &Position, len: u32) -> Option<CachedTree> {
        self.lookups += 1;
        self.last_len = Some(len);
        let k = key(pos);
        self.entries
            .iter()
            .find(|(e, _)| *e == k)
            .map(|(_, t)| t.clone())
    }

    fn store(&mut self, pos: &Position, tree: CachedTree) {
        self.stores += 1;
        let k = key(pos);
        self.entries.retain(|(e, _)| *e != k);
        self.entries.push((k, tree));
    }
}
