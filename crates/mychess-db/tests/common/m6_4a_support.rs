//! Shared helpers of the M6-4a (crosstable) tests: tournaments as PGN text
//! opened in memory.

#![allow(dead_code, unreachable_pub)]
use std::fmt::Write as _;
use std::path::Path;

use mychess_db::{CancelToken, NoProgress, OpenOptions, PgnDatabase};

/// One game of a fixture. Unset fields are left out of the PGN header.
#[derive(Clone)]
pub struct Game {
    pub event: &'static str,
    pub site: &'static str,
    pub date: &'static str,
    pub event_date: Option<&'static str>,
    pub round: &'static str,
    pub white: &'static str,
    pub black: &'static str,
    pub result: &'static str,
    pub white_elo: Option<u16>,
    pub black_elo: Option<u16>,
    pub flags: Option<&'static str>,
}

/// A game of the tournament `Club` in `Rio` on 2020.05.01.
pub fn game(white: &'static str, black: &'static str, result: &'static str) -> Game {
    Game {
        event: "Club",
        site: "Rio",
        date: "2020.05.01",
        event_date: None,
        round: "?",
        white,
        black,
        result,
        white_elo: None,
        black_elo: None,
        flags: None,
    }
}

impl Game {
    pub fn round(mut self, round: &'static str) -> Game {
        self.round = round;
        self
    }

    pub fn elo(mut self, white: u16, black: u16) -> Game {
        self.white_elo = Some(white);
        self.black_elo = Some(black);
        self
    }

    pub fn date(mut self, date: &'static str) -> Game {
        self.date = date;
        self
    }

    pub fn event(mut self, event: &'static str) -> Game {
        self.event = event;
        self
    }

    pub fn site(mut self, site: &'static str) -> Game {
        self.site = site;
        self
    }

    pub fn event_date(mut self, date: &'static str) -> Game {
        self.event_date = Some(date);
        self
    }

    pub fn deleted(mut self) -> Game {
        self.flags = Some("D");
        self
    }
}

/// The PGN text of the games.
pub fn pgn(games: &[Game]) -> String {
    let mut out = String::new();
    for g in games {
        let _ = write!(
            out,
            "[Event \"{}\"]\n[Site \"{}\"]\n[Date \"{}\"]\n[Round \"{}\"]\n[White \"{}\"]\n[Black \"{}\"]\n[Result \"{}\"]\n",
            g.event, g.site, g.date, g.round, g.white, g.black, g.result
        );
        if let Some(d) = g.event_date {
            let _ = writeln!(out, "[EventDate \"{d}\"]");
        }
        if let Some(e) = g.white_elo {
            let _ = writeln!(out, "[WhiteElo \"{e}\"]");
        }
        if let Some(e) = g.black_elo {
            let _ = writeln!(out, "[BlackElo \"{e}\"]");
        }
        if let Some(f) = g.flags {
            let _ = writeln!(out, "[ScidFlags \"{f}\"]");
        }
        let _ = writeln!(out, "\n1. e4 e5 {}\n", g.result);
    }
    out
}

/// Opens the games as an in-memory database.
pub fn open(games: &[Game]) -> PgnDatabase {
    let text = pgn(games);
    PgnDatabase::from_reader(
        text.as_bytes(),
        Path::new("fixture.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("fixture opens")
    .0
}
