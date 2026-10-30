//! Helpers of the M6-3a tests: the synthetic spelling file and small
//! databases written with `Si4Writer` (fictitious names only).
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc
)]

use std::path::{Path, PathBuf};

use mychess_core::{Elo, Game, GameResult, PartialDate, Rating, RatingType, SanMode};
use mychess_db::si4::{GameFlags, PreparedGame, Si4Database, Si4Writer};
use mychess_db::spelling::{ReadOptions, SpellingSet};

/// The example of the specification ("Dados e formatos, 1").
pub const EXAMPLE: &[u8] = include_bytes!("../fixtures/m6_3a/ssp-exemplo.ssp");

/// Options with a fixed "current year" (rule 63).
pub fn opts(year: u16) -> ReadOptions {
    ReadOptions {
        current_year: Some(year),
        ..ReadOptions::default()
    }
}

pub fn example(year: u16) -> SpellingSet {
    SpellingSet::read_bytes(EXAMPLE, &opts(year)).unwrap()
}

pub fn read(text: &str) -> SpellingSet {
    SpellingSet::read_bytes(text.as_bytes(), &opts(2020)).unwrap()
}

pub fn date(text: &str) -> PartialDate {
    PartialDate::parse_lenient(text)
}

/// One game to write.
#[derive(Clone)]
pub struct G {
    pub event: &'static str,
    pub site: &'static str,
    pub date: &'static str,
    pub event_date: &'static str,
    pub round: &'static str,
    pub white: String,
    pub black: String,
    pub white_elo: u16,
    pub black_elo: u16,
    pub result: GameResult,
    pub eco: Option<&'static str>,
    pub kind: RatingType,
}

pub fn g(white: &str, black: &str, result: GameResult) -> G {
    G {
        event: "Aberto",
        site: "Rio BRA",
        date: "????.??.??",
        event_date: "????.??.??",
        round: "?",
        white: white.to_owned(),
        black: black.to_owned(),
        white_elo: 0,
        black_elo: 0,
        result,
        eco: None,
        kind: RatingType::Elo,
    }
}

impl G {
    pub fn on(mut self, event: &'static str, site: &'static str, date: &'static str) -> G {
        self.event = event;
        self.site = site;
        self.date = date;
        self
    }
    pub fn elo(mut self, white: u16, black: u16) -> G {
        self.white_elo = white;
        self.black_elo = black;
        self
    }
    pub fn kind(mut self, kind: RatingType) -> G {
        self.kind = kind;
        self
    }
    pub fn round(mut self, round: &'static str) -> G {
        self.round = round;
        self
    }
    pub fn eco(mut self, eco: &'static str) -> G {
        self.eco = Some(eco);
        self
    }
}

fn rating(elo: u16, kind: RatingType) -> Option<Rating> {
    (elo != 0).then(|| Rating::new(Elo::new(elo).unwrap(), kind))
}

fn game_of(spec: &G) -> Game {
    let mut game = Game::new();
    let root = game.root();
    let position = game.position_at(root).unwrap().clone();
    let e4 = position.parse_san("e4", SanMode::Strict).unwrap();
    game.add_move(root, e4.mv).unwrap();
    let tags = game.tags_mut();
    tags.event = spec.event.into();
    tags.site = spec.site.into();
    tags.round = spec.round.into();
    tags.white.clone_from(&spec.white);
    tags.black.clone_from(&spec.black);
    tags.date = date(spec.date);
    tags.event_date = date(spec.event_date);
    tags.result = spec.result;
    tags.white_rating = rating(spec.white_elo, spec.kind);
    tags.black_rating = rating(spec.black_elo, spec.kind);
    game
}

/// Writes a database in `dir` and returns the path of its base.
pub fn build(dir: &Path, games: &[G]) -> PathBuf {
    let base = dir.join("m63a");
    let mut writer = Si4Writer::create(&base, 1).unwrap();
    let prepared: Vec<PreparedGame> = games
        .iter()
        .map(|spec| PreparedGame::from_game(&game_of(spec), GameFlags::empty()).unwrap())
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

/// The six-game database of the specification.
pub fn six_games() -> Vec<G> {
    use GameResult::{Black, Draw, Unknown, White};
    vec![
        g("Alfa, A", "Beta, B", White)
            .on("Aberto", "Rio BRA", "2020.01.10")
            .elo(2500, 2400),
        g("Beta, B", "Gama, G", Draw)
            .on("Aberto", "Rio BRA", "2020.01.11")
            .elo(2400, 0),
        g("Gama, G", "Alfa, A", Black)
            .on("Aberto", "Rio BRA", "2020.01.12")
            .elo(0, 2510),
        g("Alfa, A", "Beta, B", White)
            .on("Aberto", "Rio BRA", "2020.05.20")
            .elo(2520, 2410),
        g("Alfa, A", "Delta, D", Draw).on("Solo", "Lima PER", "????.??.??"),
        g("Delta, D", "Alfa, A", Unknown).on("Solo", "Lima PER", "????.??.??"),
    ]
}

pub fn index_bytes(base: &Path) -> Vec<u8> {
    std::fs::read(base.with_extension("si4")).unwrap()
}

pub fn names_bytes(base: &Path) -> Vec<u8> {
    std::fs::read(base.with_extension("sn4")).unwrap()
}

pub fn games_bytes(base: &Path) -> Vec<u8> {
    std::fs::read(base.with_extension("sg4")).unwrap()
}
