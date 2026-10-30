// Scid: src/optable.h 1-259 (`OpLine`, `OpTable`), src/optable.cpp. Copyright
// (c) 2001-2003 Shane Hudson. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The data of a report: lines, theory table, notes, statistics blocks
//! (decision D3602). Everything is plain data; the texts are made by the
//! renderers.

use mychess_core::{Eco, GameResult, PartialDate, Position};

use crate::crosstable::ResultCounts;
use crate::filter_stats::StatsLine;
use crate::tree::TreeResult;

/// Number of positional themes (Scid: `NUM_POSTHEMES`).
pub const NUM_THEMES: usize = 10;
/// A game has a theme when its positions show it at least this often
/// (Scid: `POSTHEME_THRESHOLD`).
pub const THEME_THRESHOLD: u8 = 4;
/// Half moves of each game examined for the themes: the first 20 moves
/// (Scid: `maxThemeMoveNumber * 2`).
pub const THEME_PLIES: u32 = 40;
/// Columns of the theory table, each with a White and a Black move.
pub const COLUMNS: usize = 8;
/// Most half moves a line keeps (columns plus the largest extra moves).
pub const MAX_LINE_MOVES: usize = (COLUMNS + 10) * 2;
/// Half moves of the stem kept for the header (Scid: `OPTABLE_MAX_STARTLINE`).
pub const MAX_STEM_PLIES: usize = 100;

/// The ten positional themes, in the order of the counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Theme {
    /// Both kings on the same wing.
    SameCastling,
    /// Kings on opposite wings.
    OppositeCastling,
    /// No queens left.
    QueensOff,
    /// Only one side has the bishop pair.
    OneBishopPair,
    /// A pawn storm on the kingside.
    KingsidePawnStorm,
    /// White isolated queen's pawn.
    WhiteIqp,
    /// Black isolated queen's pawn.
    BlackIqp,
    /// White pawn on the 5th, 6th or 7th rank.
    WhiteAdvancedPawn,
    /// Black pawn on the 4th, 3rd or 2nd rank.
    BlackAdvancedPawn,
    /// An open c, d or e file.
    OpenCdeFile,
}

impl Theme {
    /// All the themes, in counter order.
    pub const ALL: [Theme; NUM_THEMES] = [
        Theme::SameCastling,
        Theme::OppositeCastling,
        Theme::QueensOff,
        Theme::OneBishopPair,
        Theme::KingsidePawnStorm,
        Theme::WhiteIqp,
        Theme::BlackIqp,
        Theme::WhiteAdvancedPawn,
        Theme::BlackAdvancedPawn,
        Theme::OpenCdeFile,
    ];

    /// The index of the counter.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The class of the material at the end of a game (Scid: `EGTHEME_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EndTheme {
    /// Pawns only (or nothing else): `P`.
    #[default]
    Pawns,
    /// Minor pieces: `BN`.
    Minor,
    /// Rooks: `R`.
    Rook,
    /// Rooks and minor pieces: `R,BN`.
    RookMinor,
    /// Queens: `Q`.
    Queen,
    /// Queens and minor pieces: `Q,BN`.
    QueenMinor,
    /// Queens and rooks: `Q,R`.
    QueenRook,
    /// Queens, rooks and minor pieces: `Q,R,BN`.
    QueenRookMinor,
}

impl EndTheme {
    /// All the classes, in counter order.
    pub const ALL: [EndTheme; 8] = [
        EndTheme::Pawns,
        EndTheme::Minor,
        EndTheme::Rook,
        EndTheme::RookMinor,
        EndTheme::Queen,
        EndTheme::QueenMinor,
        EndTheme::QueenRook,
        EndTheme::QueenRookMinor,
    ];

    /// The index of the counter.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The class with counter `index`, `None` out of range.
    pub fn from_index(index: usize) -> Option<EndTheme> {
        EndTheme::ALL.get(index).copied()
    }

    /// The short name used in the tables (`P`, `BN`, `R`, `R,BN`, ...).
    pub fn short_name(self) -> &'static str {
        match self {
            EndTheme::Pawns => "P",
            EndTheme::Minor => "BN",
            EndTheme::Rook => "R",
            EndTheme::RookMinor => "R,BN",
            EndTheme::Queen => "Q",
            EndTheme::QueenMinor => "Q,BN",
            EndTheme::QueenRook => "Q,R",
            EndTheme::QueenRookMinor => "Q,R,BN",
        }
    }
}

/// One game of the report (Scid: `OpLine`).
///
/// Only the games kept for the statistics (see [`ReportCore::lines`]) have a
/// line. `moves` is filled for the lines of the theory table only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLine {
    /// The game number (1-based).
    pub game: u32,
    /// The ID of White in the name base (for the player tables).
    pub white_id: u32,
    /// The ID of Black in the name base.
    pub black_id: u32,
    /// White, as in the name base ("Surname, Given").
    pub white: String,
    /// Black.
    pub black: String,
    /// The rating of White in the index; 0 if none.
    pub white_elo: u16,
    /// The rating of Black in the index; 0 if none.
    pub black_elo: u16,
    /// The average rating that ranks the game (a missing rating is the
    /// estimate of the player, else 0; rule of `AvgElo`).
    pub avg_elo: u16,
    /// The result.
    pub result: GameResult,
    /// Half moves of the game (the index value).
    pub half_moves: u16,
    /// The date.
    pub date: PartialDate,
    /// The site.
    pub site: String,
    /// The ECO code of the game in the index.
    pub eco: Option<Eco>,
    /// The ply of the reference position in this game (the value of the
    /// tree filter minus 1).
    pub start_ply: u16,
    /// The SAN of the half moves of the theory table, without `-` and `=`
    /// (`OO`, `e8Q`); empty outside the table. With Black to move, the first
    /// one is the last move of the stem, the same for every line.
    pub moves: Vec<String>,
    /// Whether the game has no moves after the ones in `moves`.
    pub short_game: bool,
    /// How many positions of the first 20 moves show each theme.
    pub themes: [u8; NUM_THEMES],
    /// The class of the final material.
    pub end_theme: EndTheme,
    /// The ID of the move order that reaches the position (0: not counted,
    /// the list of move orders was full).
    pub move_order: u32,
    /// The number of the note that lists this game (0: none).
    pub note: u32,
}

impl ReportLine {
    /// Whether the game shows `theme` (rule of `POSTHEME_THRESHOLD`).
    pub fn has_theme(&self, theme: usize) -> bool {
        self.themes
            .get(theme)
            .is_some_and(|&n| n >= THEME_THRESHOLD)
    }

    /// The moves of the game as shown: `(half_moves + 1) / 2`.
    pub fn full_moves(&self) -> u32 {
        u32::from(self.half_moves).div_ceil(2)
    }

    /// The SAN at table index `i`, empty past the end.
    pub fn move_at(&self, i: usize) -> &str {
        self.moves.get(i).map_or("", String::as_str)
    }

    /// Number of leading half moves equal to those of `other` (Scid:
    /// `CommonLength`; an empty move ends the count).
    pub fn common_length(&self, other: &ReportLine) -> usize {
        let mut n = 0;
        while n < MAX_LINE_MOVES {
            let (a, b) = (self.move_at(n), other.move_at(n));
            if a != b || a.is_empty() {
                break;
            }
            n += 1;
        }
        n
    }

    /// The points of White: 2 for a win, 1 for a draw or no result, 0.
    pub fn white_points(&self) -> u32 {
        result_points(self.result)
    }
}

/// The points of White for a result (Scid: `RESULT_SCORE`).
pub(crate) fn result_points(result: GameResult) -> u32 {
    match result {
        GameResult::White => 2,
        GameResult::Black => 0,
        _ => 1,
    }
}

/// A row of the theory table: a main line and the lines merged into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    /// The index in [`ReportCore::lines`] of the line shown.
    pub line: usize,
    /// The lines merged into the row, as notes of the main line.
    pub merged: Vec<usize>,
    /// Games in the row (the main line and the merged ones).
    pub count: u32,
    /// Points of White in the row (2 per win, 1 per draw).
    pub points: u32,
}

impl TableRow {
    /// The score of White in the row: `(50 * points + count / 2) / count`.
    pub fn score_percent(&self) -> u32 {
        if self.count == 0 {
            return 0;
        }
        (self.points.saturating_mul(50) + self.count / 2) / self.count
    }
}

/// A line listed in a note, with the half move where its text starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteEntry {
    /// The index in [`ReportCore::lines`].
    pub line: usize,
    /// The index of the first half move shown (Scid: `NoteMoveNum`).
    pub start: usize,
}

/// A footnote of the theory table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The number, from 1.
    pub number: u32,
    /// The row that carries the marker.
    pub row: usize,
    /// The half move (0 to 15) after which the marker is printed.
    pub cell: usize,
    /// The lines of the note, in the order they are printed (the lowest in
    /// the table first, i.e. the Scid's walk from the end of the table).
    pub entries: Vec<NoteEntry>,
}

/// A way of reaching the reference position (Scid: `MoveOrder`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOrder {
    /// The ID that [`ReportLine::move_order`] refers to.
    pub id: u32,
    /// The moves from the start, numbered (`1.e4 e5 2.Nf3`).
    pub moves: String,
    /// Games that reach the position this way.
    pub count: u32,
}

/// How often the line is played in a year range, or in recent years.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YearTrend {
    /// 1, 5 or 10: the years up to today.
    pub years: u8,
    /// "Once every N games" in those years; 0 when no report game is there.
    pub ratio: u32,
    /// The change from the all-years ratio, in per cent; positive is up.
    pub delta: i32,
}

/// The popularity block (rule "Popularidade"; Scid: `setupRatios`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Popularity {
    /// "Once every N games" over all years; 0 when there are no report games.
    pub ratio_all: u32,
    /// The same for the ranges 1800-99, 1900-49, 1950-69, 1970-79, 1980-89,
    /// 1990-99, 2000-09, 2010-19 and 2020-29; `None` is the Scid's `none`.
    pub ranges: [Option<u32>; RANGE_COUNT],
    /// The last 10, 5 and 1 years.
    pub recent: [YearTrend; 3],
}

/// Number of year ranges of the popularity table.
pub const RANGE_COUNT: usize = 9;
/// The first and last year of each range of the popularity table.
pub const RANGES: [(u16, u16); RANGE_COUNT] = [
    (1800, 1899),
    (1900, 1949),
    (1950, 1969),
    (1970, 1979),
    (1980, 1989),
    (1990, 1999),
    (2000, 2009),
    (2010, 2019),
    (2020, 2029),
];

impl Default for Popularity {
    fn default() -> Popularity {
        Popularity {
            ratio_all: 0,
            ranges: [None; RANGE_COUNT],
            recent: [10u8, 5, 1].map(|years| YearTrend {
                years,
                ratio: 0,
                delta: 0,
            }),
        }
    }
}

/// Counts of the final material classes: the report games and all the games
/// of the database (rule 5 of the spec: games that end at the position count
/// only in `all`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndgameCounts {
    /// The games of the report.
    pub report: [u32; 8],
    /// Every game of the database.
    pub all: [u32; 8],
}

/// Figures of the whole database that the report compares with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DatabaseTotals {
    /// Games in the database.
    pub games: u32,
    /// Games by result.
    pub results: ResultCounts,
    /// Per result (white, black, draw, none): sum of full moves.
    pub length_sum: [u64; 4],
    /// Per result: games counted in `length_sum`.
    pub length_count: [u32; 4],
}

/// The part of a report that does not depend on how the games were chosen
/// (shared by the opening report and the player report of milestone M7-3,
/// decision D3600).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportCore {
    /// Games by result over **all** the report games (Scid: `Results`).
    pub total: ResultCounts,
    /// The same without the games of the excluded move (`TheoryResults`).
    pub theory: ResultCounts,
    /// The games kept for the statistics, by game number: those not
    /// excluded, at most [`ReportOptions::max_games`](super::ReportOptions),
    /// the highest-rated (a tie goes to the lowest number).
    pub lines: Vec<ReportLine>,
    /// Indices in `lines` of the games of the theory table, in table order.
    pub table_order: Vec<usize>,
    /// The rows of the theory table.
    pub rows: Vec<TableRow>,
    /// The footnotes of the table.
    pub notes: Vec<Note>,
    /// The statistics lines (all games, both rated, since a year).
    pub stats: Vec<StatsLine>,
    /// The popularity block.
    pub popularity: Popularity,
    /// The move orders, most frequent first.
    pub move_orders: Vec<MoveOrder>,
    /// Report games that show each theme.
    pub themes: [u32; NUM_THEMES],
    /// Final material classes.
    pub endgames: EndgameCounts,
    /// The whole database.
    pub database: DatabaseTotals,
    /// Games of the filter that could not be read and were skipped.
    pub skipped: u32,
    /// Whether the generation was cancelled: the data is what was done.
    pub interrupted: bool,
}

/// An opening report (decision D3602).
#[derive(Debug, Clone)]
pub struct OpeningReport {
    /// The moves from the start to the position, in SAN, at most
    /// [`MAX_STEM_PLIES`] (the last ones).
    pub stem: Vec<String>,
    /// How many half moves of the start of the line are not in `stem`.
    pub stem_omitted: usize,
    /// The reference position.
    pub position: Position,
    /// The ECO code of the deepest book position on the line, if any.
    pub eco: Option<String>,
    /// The move left out of the theory table, as it appears in the lines
    /// (the option text normalised), if it was found in the position.
    pub excluded: Option<String>,
    /// The shared data.
    pub core: ReportCore,
    /// The tree of the position ("Moves from the position").
    pub tree: TreeResult,
}

impl OpeningReport {
    /// Half moves of the line to the position, including those omitted.
    pub fn stem_plies(&self) -> usize {
        self.stem.len() + self.stem_omitted
    }

    /// The number of the first move of the table (`(plies + 2) / 2`).
    pub fn first_move_number(&self) -> usize {
        usize::midpoint(self.stem_plies(), 2)
    }

    /// Games in the report (the tree filter without the games that end at
    /// the position).
    pub fn game_count(&self) -> u32 {
        self.core.report_games()
    }
}
