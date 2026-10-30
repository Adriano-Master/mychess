// Scid: src/optable.cpp, src/optable.h, src/tkscid.cpp (`sc_report`),
// tcl/tools/optable.tcl. Copyright (c) 2001-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The opening report (milestone M7, slice 2a;
//! `docs/specs/m7-2-relatorio-de-abertura.md`, decisions D3600 to D3649).
//!
//! [`opening_report`] turns the tree filter of a position (the games that
//! reach it, each with the ply where it does) into an [`OpeningReport`]:
//! statistics, popularity, ratings and performance, move orders, themes,
//! endgames and the theory table with its notes. The data is plain; the text,
//! HTML and LaTeX documents are made by [`render_text`], [`render_html`] and
//! [`render_latex`], and [`select_games`] turns a click on an element into a
//! filter.
//!
//! The shared part ([`ReportCore`], made by [`build_report_core`]) is also
//! the base of the player report ([`player_report`], milestone M7-3a).
//!
//! The report never changes the filter of the user: it reads a filter that
//! the caller owns (D3601). Arithmetic is integer with the roundings of the
//! Scid, and every tie has a fixed rule, so the same input gives the same
//! report with one thread or many.

mod engine;
mod favorites;
mod labels;
mod model;
mod options;
mod player;
mod render;
mod select;
mod stats;
mod table;
mod themes;

use mychess_core::{Color, EcoBook, Move, PartialDate, Position, SanSuffix};
use thiserror::Error;

pub use engine::{CoreInput, build_report_core};
pub use favorites::{ReportFavorite, ReportFavorites, parse_reports_dat};
pub use labels::ReportLabels;
pub use model::{
    COLUMNS, DatabaseTotals, EndTheme, EndgameCounts, MAX_LINE_MOVES, MAX_STEM_PLIES, MoveOrder,
    NUM_THEMES, Note, NoteEntry, OpeningReport, Popularity, RANGE_COUNT, RANGES, ReportCore,
    ReportLine, THEME_PLIES, THEME_THRESHOLD, TableRow, Theme, YearTrend,
};
pub use options::{
    MAX_COUNT_OPTION, MAX_GAMES_LIMIT, MAX_TABLE_GAMES_LIMIT, ReportOptions, SectionSet,
};
pub use player::{
    EcoGroupShare, FrequentPlayer, NameMatch, PlayerLabels, PlayerReport, PlayerReportOptions,
    PlayerReportRequest, ReportStart, ScoreCounts, games_of_player, natural_name, player_report,
    render_player_html, render_player_latex, render_player_text,
};
pub use render::{ReportPart, render_html, render_latex, render_text};
pub use select::{Selector, select_games};
pub use stats::{BestKind, EcoGroup, EloReport, PLAYER_MAX_NOTES, PlayerRow};
pub use table::{MAX_ROWS, MIN_ROWS};

use crate::filter::Filter;
use crate::filter_stats::{StatsConfig, StatsQuery};
use crate::player_stats::PlayerStats;
use crate::progress::{CancelToken, ProgressSink};
use crate::search::SearchError;
use crate::si4::RawMove;
use crate::source::GameSource;
use crate::tree::TreeResult;

/// Why a report could not be made.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReportError {
    /// The filter does not cover the games of the database.
    #[error("the filter has {filter} games but the database has {games}")]
    FilterSize {
        /// Games in the filter.
        filter: u32,
        /// Games in the database.
        games: u32,
    },
    /// A move of the line to the position is not legal.
    #[error("move {uci} at ply {ply} of the line to the position is not legal")]
    InvalidStem {
        /// Half moves played before the move.
        ply: usize,
        /// The move, in UCI.
        uci: String,
    },
    /// The line does not end at the reference position.
    #[error("the line does not lead to the reference position")]
    StemMismatch,
    /// The name of the player has no letter or digit: empty, `?`, `,`...
    /// (player report).
    #[error("the name of the player is empty or unknown")]
    InvalidName,
    /// The selection of the games of a player was cancelled.
    #[error("the operation was cancelled")]
    Cancelled,
    /// The position of the report cannot be searched (player report).
    #[error("the position cannot be searched")]
    UnsupportedPosition,
    /// The search by position failed for another reason (player report).
    #[error("the search by position failed: {0}")]
    Search(SearchError),
}

/// Everything [`opening_report`] needs besides the database.
#[derive(Debug)]
pub struct OpeningInputs<'a> {
    /// The reference position.
    pub position: &'a Position,
    /// Where the line starts; `None` is the standard start position.
    pub start: Option<&'a Position>,
    /// The moves from the start to `position`.
    pub stem: &'a [Move],
    /// The tree filter of the position (`TreeState::tree_filter`): the
    /// value of a game is the ply of the position in it, plus one.
    pub tree_filter: &'a Filter,
    /// The tree of the position, for "Moves from the position".
    pub tree: TreeResult,
    /// The options.
    pub options: &'a ReportOptions,
    /// The ECO book, for the code in the header.
    pub eco_book: Option<&'a EcoBook>,
    /// The estimated ratings.
    pub player_stats: &'a PlayerStats,
    /// The lines of the statistics block; `None` is the default of the
    /// statistics window for the year of `today`.
    pub stats_config: Option<&'a StatsConfig>,
    /// Today (D3608): the clock is read by the caller.
    pub today: PartialDate,
}

fn board_key(pos: &Position) -> String {
    pos.fen().split(' ').take(2).collect::<Vec<_>>().join(" ")
}

fn strip_marks(text: &str) -> String {
    text.chars().filter(|c| *c != '-' && *c != '=').collect()
}

/// Plays `stem` from `start` (the standard start when `None`) and checks
/// that it ends at `position`. Returns every position of the line, the start
/// included, and the SAN of each move.
fn walk_stem(
    start: Option<&Position>,
    stem: &[Move],
    position: &Position,
) -> Result<(Vec<Position>, Vec<String>), ReportError> {
    let mut pos = start.cloned().unwrap_or_default();
    let mut positions = vec![pos.clone()];
    let mut sans: Vec<String> = Vec::with_capacity(stem.len());
    for (ply, &mv) in stem.iter().enumerate() {
        let invalid = || ReportError::InvalidStem {
            ply,
            uci: pos.uci(mv),
        };
        let san = pos
            .san_with(mv, SanSuffix::CheckOrMate)
            .map_err(|_| invalid())?;
        let next = pos.after(mv).map_err(|_| invalid())?;
        sans.push(san);
        pos = next;
        positions.push(pos.clone());
    }
    if board_key(&pos) != board_key(position) {
        return Err(ReportError::StemMismatch);
    }
    Ok((positions, sans))
}

/// Scid: the code of the deepest position of the line that is in the book;
/// the start position is not examined.
fn deepest_eco(book: Option<&EcoBook>, positions: &[Position]) -> Option<String> {
    book.and_then(|book| {
        positions
            .iter()
            .skip(1)
            .rev()
            .find_map(|p| book.lookup(p))
            .map(|entry| entry.eco().to_string())
    })
}

/// The lines of the statistics block that a report shows: all games, both
/// rated and since a year.
fn report_stats_queries(config: Option<&StatsConfig>, today: PartialDate) -> Vec<StatsQuery> {
    match config {
        Some(cfg) => cfg.queries(),
        None => StatsConfig::new(today.year().unwrap_or(0)).queries(),
    }
    .into_iter()
    .filter(|q| {
        matches!(
            q,
            StatsQuery::All | StatsQuery::BothRated(_) | StatsQuery::Since(_)
        )
    })
    .collect()
}

/// The move of `pos` that the text of the option "exclude" names, as the
/// index stores it, and its text as shown in the table.
fn resolve_exclude(pos: &Position, text: &str) -> Option<(RawMove, String)> {
    let wanted = strip_marks(text.trim());
    pos.legal_moves().into_iter().find_map(|m| {
        let with = strip_marks(&pos.san_with(m, SanSuffix::CheckOrMate).ok()?);
        let without = strip_marks(&pos.san_with(m, SanSuffix::None).ok()?);
        if wanted != with && wanted != without {
            return None;
        }
        Some((
            RawMove {
                from: m.from()?,
                to: m.to()?,
                promotion: m.promotion(),
            },
            with,
        ))
    })
}

/// Makes the opening report of a position (`sc_report opening create`).
///
/// Progress runs from 0 to the sum of the games of the database and of the
/// report, once, and never goes back. When `cancel` is set the report is
/// what was done so far, with `core.interrupted` set.
///
/// # Errors
///
/// [`ReportError::FilterSize`] for a filter of another database;
/// [`ReportError::InvalidStem`] and [`ReportError::StemMismatch`] when the
/// line does not lead to the position.
pub fn opening_report<D: GameSource + ?Sized>(
    db: &mut D,
    inputs: OpeningInputs<'_>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<OpeningReport, ReportError> {
    let options = inputs.options.normalized();
    let (positions, mut sans) = walk_stem(inputs.start, inputs.stem, inputs.position)?;
    let eco = deepest_eco(inputs.eco_book, &positions);

    let excluded = options
        .exclude
        .as_deref()
        .and_then(|text| resolve_exclude(inputs.position, text));
    let stem_last = (inputs.position.turn() == Color::Black)
        .then(|| sans.last().map(|s| strip_marks(s)))
        .flatten();
    let queries = report_stats_queries(inputs.stats_config, inputs.today);

    let core = build_report_core(
        db,
        &CoreInput {
            filter: inputs.tree_filter,
            turn: inputs.position.turn(),
            stem_last: stem_last.as_deref(),
            exclude: excluded.as_ref().map(|(raw, _)| *raw),
            options: &options,
            player_stats: inputs.player_stats,
            stats_queries: &queries,
            today: inputs.today,
        },
        progress,
        cancel,
    )?;

    let omitted = sans.len().saturating_sub(MAX_STEM_PLIES);
    let stem = sans.split_off(omitted);
    Ok(OpeningReport {
        stem,
        stem_omitted: omitted,
        position: inputs.position.clone(),
        eco,
        excluded: excluded.map(|(_, text)| text),
        core,
        tree: inputs.tree,
    })
}
