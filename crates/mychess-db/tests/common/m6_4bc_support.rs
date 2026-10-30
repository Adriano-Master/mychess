//! Shared helpers of the M6-4b and M6-4c tests: games as PGN text, opened as
//! an in-memory database.

#![allow(dead_code, unreachable_pub, clippy::assigning_clones)]
use std::fmt::Write as _;
use std::path::Path;

use mychess_db::{CancelToken, Filter, NoProgress, OpenOptions, PgnDatabase};

/// A short, legal opening line (20 plies) the fixtures cut to any length.
pub const LINE: [&str; 20] = [
    "e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4", "Nf6", "O-O", "Be7", "Re1", "b5", "Bb3", "d6",
    "c3", "O-O", "h3", "Nb8", "d4", "Nbd7",
];

/// The movetext of the first `plies` plies of [`LINE`].
pub fn line(plies: usize) -> String {
    movetext(&LINE[..plies.min(LINE.len())])
}

/// Movetext (`1. e4 e5 2. Nf3 ...`) from SAN moves.
pub fn movetext(sans: &[&str]) -> String {
    let mut out = String::new();
    for (i, san) in sans.iter().enumerate() {
        if i % 2 == 0 {
            let _ = write!(out, "{}. ", i / 2 + 1);
        }
        let _ = write!(out, "{san} ");
    }
    out
}

/// One game of a fixture.
#[derive(Clone)]
pub struct G {
    pub white: String,
    pub black: String,
    pub result: &'static str,
    pub date: String,
    pub white_elo: Option<u16>,
    pub black_elo: Option<u16>,
    pub moves: String,
    pub fen: Option<String>,
}

pub fn g(white: &str, black: &str, result: &'static str) -> G {
    G {
        white: white.to_owned(),
        black: black.to_owned(),
        result,
        date: "2000.01.01".to_owned(),
        white_elo: None,
        black_elo: None,
        moves: line(4),
        fen: None,
    }
}

impl G {
    pub fn date(mut self, date: &str) -> G {
        self.date = date.to_owned();
        self
    }

    pub fn elo(mut self, white: u16, black: u16) -> G {
        self.white_elo = Some(white);
        self.black_elo = Some(black);
        self
    }

    pub fn plies(mut self, plies: usize) -> G {
        self.moves = line(plies);
        self
    }

    pub fn moves(mut self, text: &str) -> G {
        self.moves = text.to_owned();
        self
    }

    pub fn fen(mut self, fen: &str) -> G {
        self.fen = Some(fen.to_owned());
        self
    }
}

/// The PGN text of the games.
pub fn pgn(games: &[G]) -> String {
    let mut out = String::new();
    for g in games {
        let _ = write!(
            out,
            "[Event \"E\"]\n[Site \"S\"]\n[Date \"{}\"]\n[Round \"1\"]\n[White \"{}\"]\n[Black \"{}\"]\n[Result \"{}\"]\n",
            g.date, g.white, g.black, g.result
        );
        if let Some(e) = g.white_elo {
            let _ = writeln!(out, "[WhiteElo \"{e}\"]");
        }
        if let Some(e) = g.black_elo {
            let _ = writeln!(out, "[BlackElo \"{e}\"]");
        }
        if let Some(fen) = &g.fen {
            let _ = writeln!(out, "[SetUp \"1\"]\n[FEN \"{fen}\"]");
        }
        let _ = writeln!(out, "\n{}{}\n", g.moves, g.result);
    }
    out
}

/// Opens PGN text as an in-memory database.
pub fn open_text(text: &str) -> PgnDatabase {
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

/// Opens the games as an in-memory database.
pub fn open(games: &[G]) -> PgnDatabase {
    open_text(&pgn(games))
}

/// A filter with every game of the database.
pub fn all(db: &PgnDatabase) -> Filter {
    use mychess_db::GameSource;
    let mut filter = Filter::new(db.len());
    filter.fill(1);
    filter
}

/// A filter with exactly the given game numbers.
pub fn only(db: &PgnDatabase, games: &[u32]) -> Filter {
    use mychess_db::GameSource;
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    for &n in games {
        filter.set(n, 1);
    }
    filter
}
