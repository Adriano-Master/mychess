//! Helpers of the M7-3a tests (player report): the "report database" of the
//! M7-2 spec with the ECO codes and the game 8 of the M7-3 spec, as PGN text
//! in memory, and one-call builders. Included with `#[path]` by each test
//! file.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::too_many_arguments
)]

use std::fmt::Write as _;
use std::path::Path;

use mychess_core::{Color, Move, PartialDate, Position};
use mychess_db::report::{
    NameMatch, PlayerReport, PlayerReportOptions, PlayerReportRequest, ReportStart, player_report,
};
use mychess_db::si4::{Header, IndexEntry, NameBase, RawGame, Si4Error};
use mychess_db::{
    CancelToken, GameSource, NoProgress, OpenOptions, PgnDatabase, PlayerStats, ProgressSink,
};

/// One PGN game with an optional ECO tag.
pub fn game(
    white: &str,
    black: &str,
    result: &str,
    date: &str,
    site: &str,
    elos: (u16, u16),
    eco: Option<&str>,
    moves: &str,
) -> String {
    let mut s = format!(
        "[Event \"?\"]\n[Site \"{site}\"]\n[Date \"{date}\"]\n[Round \"?\"]\n[White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n"
    );
    if elos.0 > 0 {
        let _ = writeln!(s, "[WhiteElo \"{}\"]", elos.0);
    }
    if elos.1 > 0 {
        let _ = writeln!(s, "[BlackElo \"{}\"]", elos.1);
    }
    if let Some(eco) = eco {
        let _ = writeln!(s, "[ECO \"{eco}\"]");
    }
    let _ = write!(s, "\n{moves} {result}\n\n");
    s
}

/// The seven games of the report database of M7-2 with their ECO codes.
pub fn seven_games() -> Vec<String> {
    vec![
        game(
            "Alfa, A",
            "Beta, B",
            "1-0",
            "2020.01.10",
            "Rio",
            (2500, 2400),
            Some("C78"),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7",
        ),
        game(
            "Gama, G",
            "Alfa, A",
            "1/2-1/2",
            "2015.05.02",
            "Lima",
            (2300, 2500),
            Some("C50"),
            "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5",
        ),
        game(
            "Beta, B",
            "Gama, G",
            "0-1",
            "2018.07.07",
            "Rio",
            (2400, 2300),
            Some("C60"),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6",
        ),
        game(
            "Alfa, A",
            "Delta, D",
            "1-0",
            "????.??.??",
            "Lima",
            (2500, 0),
            Some("C41"),
            "1. e4 e5 2. Nf3 d6 3. d4",
        ),
        game(
            "Delta, D",
            "Beta, B",
            "*",
            "2021.01.01",
            "Rio",
            (0, 2400),
            Some("C40"),
            "1. e4 e5 2. Nf3",
        ),
        game(
            "Alfa, A",
            "Gama, G",
            "1-0",
            "2019.03.03",
            "Rio",
            (2500, 2300),
            Some("C60"),
            "1. Nf3 e5 2. e4 Nc6 3. Bb5",
        ),
        game(
            "Gama, G",
            "Alfa, A",
            "0-1",
            "2017.06.06",
            "Rio",
            (2300, 2500),
            Some("D06"),
            "1. d4 d5 2. c4",
        ),
    ]
}

/// The game 8 of the M7-3 spec: `Alfa, Z` (1000) x `Beta, B` (2400).
pub fn game_eight() -> String {
    game(
        "Alfa, Z",
        "Beta, B",
        "1-0",
        "2022.01.01",
        "Rio",
        (1000, 2400),
        Some("B20"),
        "1. e4 c5",
    )
}

/// The eight games of the fixture of the M7-3 spec, as PGN.
pub fn fixture_pgn() -> String {
    let mut games = seven_games();
    games.push(game_eight());
    games.concat()
}

pub fn open(pgn: &str) -> PgnDatabase {
    PgnDatabase::from_reader(
        pgn.as_bytes(),
        Path::new("report.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .map(|(db, _)| db)
    .unwrap()
}

pub fn fixture() -> PgnDatabase {
    open(&fixture_pgn())
}

/// The position after the UCI moves from the start, and the moves.
pub fn line(ucis: &str) -> (Position, Vec<Move>) {
    let mut pos = Position::new();
    let mut moves = Vec::new();
    for u in ucis.split_whitespace() {
        let mv = pos.parse_uci(u).unwrap();
        pos.play(mv).unwrap();
        moves.push(mv);
    }
    (pos, moves)
}

pub const E4_E5_NF3: &str = "e2e4 e7e5 g1f3";

pub fn today() -> PartialDate {
    PartialDate::parse_lenient("2026.10.29")
}

/// What to ask for.
#[derive(Clone)]
pub struct Ask<'a> {
    pub name: &'a str,
    pub color: Color,
    pub matching: NameMatch,
    pub ucis: Option<&'a str>,
    pub options: PlayerReportOptions,
}

pub fn ask(name: &str, color: Color) -> Ask<'_> {
    Ask {
        name,
        color,
        matching: NameMatch::Exact,
        ucis: None,
        options: PlayerReportOptions::default(),
    }
}

/// The player report of `ask`.
pub fn run<D: GameSource + ?Sized>(db: &mut D, ask: &Ask<'_>) -> PlayerReport {
    run_with(db, ask, &mut NoProgress, &CancelToken::new()).unwrap()
}

pub fn run_with<D: GameSource + ?Sized>(
    db: &mut D,
    ask: &Ask<'_>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<PlayerReport, mychess_db::report::ReportError> {
    let stats = PlayerStats::from_database(db);
    let position = ask.ucis.map(line);
    let start = match &position {
        Some((pos, moves)) => ReportStart::Position(pos, moves),
        None => ReportStart::Beginning,
    };
    player_report(
        db,
        &PlayerReportRequest {
            name: ask.name,
            color: ask.color,
            matching: ask.matching,
            start,
            options: &ask.options,
            eco_book: None,
            player_stats: &stats,
            stats_config: None,
            today: today(),
        },
        progress,
        cancel,
    )
}

/// The numbers of the games of the lines of a report.
pub fn line_games(report: &PlayerReport) -> Vec<u32> {
    report.core.lines.iter().map(|l| l.game).collect()
}

/// A database whose game `broken` cannot be read (its record fails), to see
/// that unreadable games are skipped and counted.
pub struct Flaky {
    pub inner: PgnDatabase,
    pub broken: u32,
}

impl GameSource for Flaky {
    fn header(&self) -> &Header {
        self.inner.header()
    }
    fn len(&self) -> u32 {
        self.inner.len()
    }
    fn entry(&self, n: u32) -> mychess_db::si4::Result<IndexEntry> {
        self.inner.entry(n)
    }
    fn names(&self) -> &NameBase {
        self.inner.names()
    }
    fn record_bytes(&mut self, n: u32) -> mychess_db::si4::Result<&[u8]> {
        if n == self.broken {
            return Err(Si4Error::Io {
                path: "broken".into(),
                source: std::io::Error::other("test"),
            });
        }
        self.inner.record_bytes(n)
    }
    fn raw_game(&mut self, n: u32) -> mychess_db::si4::Result<RawGame> {
        self.inner.raw_game(n)
    }
    fn game(&mut self, n: u32) -> mychess_db::si4::Result<mychess_core::Game> {
        self.inner.game(n)
    }
}

/// A database whose index entry `broken` cannot be read.
pub struct BadEntry {
    pub inner: PgnDatabase,
    pub broken: u32,
}

impl GameSource for BadEntry {
    fn header(&self) -> &Header {
        self.inner.header()
    }
    fn len(&self) -> u32 {
        self.inner.len()
    }
    fn entry(&self, n: u32) -> mychess_db::si4::Result<IndexEntry> {
        if n == self.broken {
            return Err(Si4Error::Io {
                path: "broken".into(),
                source: std::io::Error::other("test"),
            });
        }
        self.inner.entry(n)
    }
    fn names(&self) -> &NameBase {
        self.inner.names()
    }
    fn record_bytes(&mut self, n: u32) -> mychess_db::si4::Result<&[u8]> {
        self.inner.record_bytes(n)
    }
    fn raw_game(&mut self, n: u32) -> mychess_db::si4::Result<RawGame> {
        self.inner.raw_game(n)
    }
    fn game(&mut self, n: u32) -> mychess_db::si4::Result<mychess_core::Game> {
        self.inner.game(n)
    }
}
