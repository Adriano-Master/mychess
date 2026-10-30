//! Helpers of the M3-3a contract tests (filter and header search). Games are
//! assembled in code and written with `Si4Writer`; entries are patched with
//! raw bytes (layout from `docs/formatos/si4.md`) to create damaged files.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::missing_panics_doc
)]

use std::path::{Path, PathBuf};

use mychess_core::{Eco, Elo, Game, GameResult, PartialDate, Position, Rating, RatingType};
use mychess_db::si4::{GameFlags, PreparedGame, Si4Database, Si4Writer};
use mychess_db::{
    CancelToken, Filter, HeaderSearch, NoProgress, PlayerStats, SearchReport, search_header,
};

pub const HEADER_LEN: usize = 182;
pub const ENTRY_LEN: usize = 47;

pub fn play(game: &mut Game, parent: mychess_core::NodeId, uci: &str) -> mychess_core::NodeId {
    let mv = game.position_at(parent).unwrap().parse_uci(uci).unwrap();
    game.add_move(parent, mv).unwrap()
}

pub fn line(game: &mut Game, ucis: &[&str]) {
    let mut at = game.root();
    for u in ucis {
        at = play(game, at, u);
    }
}

/// Game builder. `B::new` plays `1.e4 e5` (2 half moves, White to move).
pub struct B(pub Game);

impl B {
    pub fn new(white: &str, black: &str) -> B {
        let mut g = Game::new();
        line(&mut g, &["e2e4", "e7e5"]);
        let t = g.tags_mut();
        t.white = white.into();
        t.black = black.into();
        B(g)
    }
    pub fn plies(white: &str, black: &str, n: usize) -> B {
        const CYCLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let mut g = Game::new();
        let mut at = g.root();
        for i in 0..n {
            at = play(&mut g, at, CYCLE[i % 4]);
        }
        let t = g.tags_mut();
        t.white = white.into();
        t.black = black.into();
        B(g)
    }
    pub fn of(game: Game) -> B {
        B(game)
    }
    pub fn result(mut self, r: GameResult) -> B {
        self.0.tags_mut().result = r;
        self
    }
    pub fn date(mut self, d: &str) -> B {
        self.0.tags_mut().date = PartialDate::parse_lenient(d);
        self
    }
    pub fn event(mut self, s: &str) -> B {
        self.0.tags_mut().event = s.into();
        self
    }
    pub fn site(mut self, s: &str) -> B {
        self.0.tags_mut().site = s.into();
        self
    }
    pub fn round(mut self, s: &str) -> B {
        self.0.tags_mut().round = s.into();
        self
    }
    pub fn elo(mut self, white: u16, black: u16) -> B {
        let r = |v: u16| {
            Elo::new(v)
                .ok()
                .filter(|_| v != 0)
                .map(|e| Rating::new(e, RatingType::Elo))
        };
        let t = self.0.tags_mut();
        t.white_rating = r(white);
        t.black_rating = r(black);
        self
    }
    pub fn eco(mut self, code: &str) -> B {
        self.0.tags_mut().eco = Eco::parse_prefix(code).map(|(e, _)| e);
        assert!(self.0.tags().eco.is_some(), "bad eco {code}");
        self
    }
    pub fn extra(mut self, name: &str, value: &str) -> B {
        self.0.tags_mut().set_extra(name, value).unwrap();
        self
    }
    pub fn done(self) -> (Game, GameFlags) {
        (self.0, GameFlags::empty())
    }
    pub fn flags(self, flags: GameFlags) -> (Game, GameFlags) {
        (self.0, flags)
    }
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub base: PathBuf,
}

impl Fixture {
    pub fn open(&self) -> Si4Database {
        Si4Database::open(&self.base).unwrap()
    }
    pub fn si4(&self) -> PathBuf {
        self.base.with_extension("si4")
    }
}

pub fn build(games: &[(Game, GameFlags)]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("games");
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let prepared: Vec<PreparedGame> = games
        .iter()
        .map(|(g, f)| PreparedGame::from_game(g, *f).unwrap())
        .collect();
    w.add_prepared_batch(&prepared).unwrap();
    drop(w);
    Fixture { dir, base }
}

/// Patches the 47 bytes of entry `n` (1-based) in the `.si4`.
pub fn patch_entry(fx: &Fixture, n: u32, f: impl FnOnce(&mut [u8])) {
    let mut bytes = std::fs::read(fx.si4()).unwrap();
    let at = HEADER_LEN + ENTRY_LEN * (n as usize - 1);
    f(&mut bytes[at..at + ENTRY_LEN]);
    std::fs::write(fx.si4(), bytes).unwrap();
}

/// Makes the record of game `n` empty (`Length = 0`).
pub fn zero_length(fx: &Fixture, n: u32) {
    patch_entry(fx, n, |e| {
        e[4] = 0;
        e[5] = 0;
        e[6] &= 0x7F;
    });
}

/// Points the record of game `n` far beyond the end of the `.sg4`.
pub fn break_offset(fx: &Fixture, n: u32) {
    patch_entry(fx, n, |e| {
        e[0..4].copy_from_slice(&0x7FFF_FF00_u32.to_be_bytes());
    });
}

/// Makes the entry itself undecodable (result nibble above 3).
pub fn break_entry(fx: &Fixture, n: u32) {
    patch_entry(fx, n, |e| e[21] |= 0xF0);
}

/// Runs `q` with a fresh all-in filter and returns the matching numbers.
pub fn hits(fx: &Fixture, q: &HeaderSearch) -> Vec<u32> {
    hits_report(fx, q).0
}

pub fn hits_report(fx: &Fixture, q: &HeaderSearch) -> (Vec<u32>, SearchReport) {
    let mut db = fx.open();
    let stats = PlayerStats::from_database(&db);
    let mut filter = Filter::new(db.len());
    let report = search_header(
        &mut db,
        q,
        &mut filter,
        &stats,
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    (filter.iter_games().collect(), report)
}

/// Runs `q` over an existing filter.
pub fn run(fx: &Fixture, q: &HeaderSearch, filter: &mut Filter) -> SearchReport {
    let mut db = fx.open();
    let stats = PlayerStats::from_database(&db);
    search_header(
        &mut db,
        q,
        filter,
        &stats,
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

pub fn q() -> HeaderSearch {
    HeaderSearch::default()
}

pub fn fen_game(fen: &str, ucis: &[&str]) -> Game {
    let mut g = Game::from_position(Position::from_fen(fen).unwrap());
    line(&mut g, ucis);
    g
}

/// 1.f3 e5 2.g4 Qh4#
pub fn fools_mate() -> Game {
    let mut g = Game::new();
    line(&mut g, &["f2f3", "e7e5", "g2g4", "d8h4"]);
    g
}

/// From a stalemate-in-one position: 1.Qf7 (black king h8 is stalemated).
pub fn stalemate_game() -> Game {
    let g = fen_game("7k/8/8/5Q2/8/8/8/1K6 w - - 0 1", &["f5f7"]);
    let pos = g.position_at(g.mainline().last().unwrap()).unwrap();
    assert!(
        pos.is_stalemate() && !pos.is_check(),
        "setup must stalemate"
    );
    g
}

pub fn path_exists(p: &Path) -> bool {
    p.exists()
}
