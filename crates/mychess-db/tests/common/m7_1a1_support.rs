//! Helpers of the M7-1a1 tests: small SI4 databases written with `Si4Writer`
//! from a compact game description (fictitious names only).
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc
)]

use std::path::{Path, PathBuf};

use mychess_core::{Elo, Game, GameResult, PartialDate, Rating, RatingType};
use mychess_db::si4::{GameFlags, PreparedGame, Si4Database, Si4Writer};

/// One game to write.
#[derive(Clone, Debug)]
pub struct S {
    pub white: String,
    pub black: String,
    pub event: String,
    pub site: String,
    pub round: String,
    pub date: String,
    pub result: GameResult,
    pub white_elo: u16,
    pub black_elo: u16,
    pub moves: Vec<&'static str>,
    pub comment: bool,
    pub variation: bool,
    pub eco: Option<&'static str>,
    pub flags: GameFlags,
}

pub const RUY6: [&str; 6] = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"];
pub const RUY8: [&str; 8] = [
    "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6",
];
pub const QG4: [&str; 4] = ["d2d4", "d7d5", "c2c4", "e7e6"];

/// A game of the twin fixture: Alfa x Beta, Aberto, Rio, round 1, 2020.01.10.
pub fn s(moves: &[&'static str]) -> S {
    S {
        white: "Alfa, A".into(),
        black: "Beta, B".into(),
        event: "Aberto".into(),
        site: "Rio".into(),
        round: "1".into(),
        date: "2020.01.10".into(),
        result: GameResult::White,
        white_elo: 0,
        black_elo: 0,
        moves: moves.to_vec(),
        comment: false,
        variation: false,
        eco: None,
        flags: GameFlags::empty(),
    }
}

impl S {
    pub fn players(mut self, white: &str, black: &str) -> S {
        self.white = white.into();
        self.black = black.into();
        self
    }
    pub fn event(mut self, event: &str) -> S {
        self.event = event.into();
        self
    }
    pub fn site(mut self, site: &str) -> S {
        self.site = site.into();
        self
    }
    pub fn round(mut self, round: &str) -> S {
        self.round = round.into();
        self
    }
    pub fn date(mut self, date: &str) -> S {
        self.date = date.into();
        self
    }
    pub fn result(mut self, result: GameResult) -> S {
        self.result = result;
        self
    }
    pub fn elo(mut self, white: u16, black: u16) -> S {
        self.white_elo = white;
        self.black_elo = black;
        self
    }
    pub fn commented(mut self) -> S {
        self.comment = true;
        self
    }
    pub fn with_variation(mut self) -> S {
        self.variation = true;
        self
    }
    pub fn flags(mut self, flags: GameFlags) -> S {
        self.flags = flags;
        self
    }
}

fn rating(elo: u16) -> Option<Rating> {
    (elo != 0).then(|| Rating::new(Elo::new(elo).unwrap(), RatingType::Elo))
}

pub fn game_of(spec: &S) -> Game {
    let mut game = Game::new();
    let mut at = game.root();
    let mut first = None;
    for uci in &spec.moves {
        let mv = game.position_at(at).unwrap().parse_uci(uci).unwrap();
        at = game.add_move(at, mv).unwrap();
        first.get_or_insert(at);
    }
    if let Some(first) = first {
        if spec.comment {
            game.set_comment(first, "a comment").unwrap();
        }
        if spec.variation {
            let alt = game
                .position_at(game.root())
                .unwrap()
                .parse_uci("a2a3")
                .unwrap();
            game.add_move(game.root(), alt).unwrap();
        }
    }
    let tags = game.tags_mut();
    tags.event.clone_from(&spec.event);
    tags.site.clone_from(&spec.site);
    tags.round.clone_from(&spec.round);
    tags.white.clone_from(&spec.white);
    tags.black.clone_from(&spec.black);
    tags.date = PartialDate::parse_lenient(&spec.date);
    tags.result = spec.result;
    tags.white_rating = rating(spec.white_elo);
    tags.black_rating = rating(spec.black_elo);
    game
}

/// Writes a database named `m71a1` in `dir` and returns its base path.
pub fn build(dir: &Path, games: &[S]) -> PathBuf {
    let base = dir.join("m71a1");
    let mut writer = Si4Writer::create(&base, 1).unwrap();
    let prepared: Vec<PreparedGame> = games
        .iter()
        .map(|spec| PreparedGame::from_game(&game_of(spec), spec.flags).unwrap())
        .collect();
    let range = writer.add_prepared_batch(&prepared).unwrap();
    let ecos: Vec<(u32, Option<mychess_core::Eco>)> = range
        .zip(games)
        .filter_map(|(n, g)| g.eco.map(|e| (n, Some(e.parse().unwrap()))))
        .collect();
    writer.set_eco_batch(&ecos).unwrap();
    drop(writer);
    base
}

pub fn open(base: &Path) -> Si4Database<std::fs::File> {
    Si4Database::open(base).unwrap()
}

/// The index file (`.si4`) bytes.
pub fn index_bytes(base: &Path) -> Vec<u8> {
    std::fs::read(base.with_extension("si4")).unwrap()
}

/// The 47 bytes of every entry.
pub fn entries_of(index: &[u8]) -> Vec<&[u8]> {
    index[182..]
        .as_chunks::<47>()
        .0
        .iter()
        .map(<[u8; 47]>::as_slice)
        .collect()
}

/// The five-game twin database of the specification.
pub fn twin_games() -> Vec<S> {
    vec![
        s(&RUY6),
        s(&RUY8),
        s(&QG4),
        s(&RUY6).commented(),
        s(&RUY6).date("2020.02.11"),
    ]
}

/// The six-game database of M6-3.
pub fn six_games() -> Vec<S> {
    use GameResult::{Black, Draw, Unknown, White};
    let g = |w: &str, b: &str, r, site: &str, ev: &str, date: &str, we, be| {
        s(&["e2e4"])
            .players(w, b)
            .result(r)
            .site(site)
            .event(ev)
            .date(date)
            .elo(we, be)
    };
    vec![
        g(
            "Alfa, A",
            "Beta, B",
            White,
            "Rio BRA",
            "Aberto",
            "2020.01.10",
            2500,
            2400,
        ),
        g(
            "Beta, B",
            "Gama, G",
            Draw,
            "Rio BRA",
            "Aberto",
            "2020.01.11",
            2400,
            0,
        ),
        g(
            "Gama, G",
            "Alfa, A",
            Black,
            "Rio BRA",
            "Aberto",
            "2020.01.12",
            0,
            2510,
        ),
        g(
            "Alfa, A",
            "Beta, B",
            White,
            "Rio BRA",
            "Aberto",
            "2020.05.20",
            2520,
            2410,
        ),
        g(
            "Alfa, A",
            "Delta, D",
            Draw,
            "Lima PER",
            "Solo",
            "????.??.??",
            0,
            0,
        ),
        g(
            "Delta, D",
            "Alfa, A",
            Unknown,
            "Lima PER",
            "Solo",
            "????.??.??",
            0,
            0,
        ),
    ]
}
