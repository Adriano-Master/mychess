//! Helpers of the M7-2a tests (opening report): the "report database" of the
//! spec as PGN text in memory, and a one-call report builder. Included with
//! `#[path]` by each test file.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::too_many_arguments
)]

use std::fmt::Write as _;
use std::path::Path;

use mychess_core::{Move, PartialDate, Position};
use mychess_db::report::{OpeningInputs, OpeningReport, ReportOptions, opening_report};
use mychess_db::tree::{NoCache, TreeOptions, TreeQuery, TreeState, search};
use mychess_db::{CancelToken, NoProgress, OpenOptions, PgnDatabase, PlayerStats, ProgressSink};

/// One PGN game.
pub fn pgn_game(
    white: &str,
    black: &str,
    result: &str,
    date: &str,
    site: &str,
    elos: (u16, u16),
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
    let _ = write!(s, "\n{moves} {result}\n\n");
    s
}

/// The seven games of the report database of the spec.
pub fn fixture_pgn() -> String {
    [
        pgn_game(
            "Alfa, A",
            "Beta, B",
            "1-0",
            "2020.01.10",
            "Rio",
            (2500, 2400),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7",
        ),
        pgn_game(
            "Gama, G",
            "Alfa, A",
            "1/2-1/2",
            "2015.05.02",
            "Lima",
            (2300, 2500),
            "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5",
        ),
        pgn_game(
            "Beta, B",
            "Gama, G",
            "0-1",
            "2018.07.07",
            "Rio",
            (2400, 2300),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6",
        ),
        pgn_game(
            "Alfa, A",
            "Delta, D",
            "1-0",
            "????.??.??",
            "Lima",
            (2500, 0),
            "1. e4 e5 2. Nf3 d6 3. d4",
        ),
        pgn_game(
            "Delta, D",
            "Beta, B",
            "*",
            "2021.01.01",
            "Rio",
            (0, 2400),
            "1. e4 e5 2. Nf3",
        ),
        pgn_game(
            "Alfa, A",
            "Gama, G",
            "1-0",
            "2019.03.03",
            "Rio",
            (2500, 2300),
            "1. Nf3 e5 2. e4 Nc6 3. Bb5",
        ),
        pgn_game(
            "Gama, G",
            "Alfa, A",
            "0-1",
            "2017.06.06",
            "Rio",
            (2300, 2500),
            "1. d4 d5 2. c4",
        ),
    ]
    .concat()
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

/// The report of the position after `ucis` with a fresh tree search.
pub fn report(db: &mut PgnDatabase, ucis: &str, opts: &ReportOptions) -> OpeningReport {
    report_with(db, ucis, opts, &mut NoProgress, &CancelToken::new())
}

pub fn report_with(
    db: &mut PgnDatabase,
    ucis: &str,
    opts: &ReportOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> OpeningReport {
    let (pos, stem) = line(ucis);
    let main = db.new_filter();
    let mut state = TreeState::default();
    let tree = search(
        db,
        &TreeQuery {
            position: &pos,
            ply: u32::try_from(stem.len()).unwrap(),
            main_filter: &main,
        },
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    let stats = PlayerStats::from_database(db);
    let filter = state.tree_filter().unwrap().clone();
    opening_report(
        db,
        OpeningInputs {
            position: &pos,
            start: None,
            stem: &stem,
            tree_filter: &filter,
            tree,
            options: opts,
            eco_book: None,
            player_stats: &stats,
            stats_config: None,
            today: today(),
        },
        progress,
        cancel,
    )
    .unwrap()
}

/// The numbers of the games of the lines of a report.
pub fn line_games(report: &OpeningReport) -> Vec<u32> {
    report.core.lines.iter().map(|l| l.game).collect()
}
