//! The piece tracker as the front-end draws it (spec M6-4, slice c, D3389,
//! D3409): the heat map with the colours already computed.

use mychess_core::Square;
use mychess_db::PieceTrackResult;

use super::{PieceTrackerNotice, PieceTrackerPrefs, TrackerMode, TrackerScheme};

/// A square of the map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackerCell {
    /// The square.
    pub square: Square,
    /// The count.
    pub count: u32,
    /// The share in per cent with two decimals (`ptracker.tcl` 354-359).
    pub percent: f64,
    /// `count / largest count`, 0 to 1.
    pub fraction: f32,
    /// The colour of the scheme.
    pub rgb: [u8; 3],
}

/// A line of the list of the 64 squares.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackerRank {
    /// The position, from 1.
    pub position: usize,
    /// The square.
    pub square: Square,
    /// Its name (`e4`).
    pub name: String,
    /// The count.
    pub count: u32,
    /// The share in per cent.
    pub percent: f64,
}

/// The panel, ready to draw. The map is never turned: White at the bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceTrackerView {
    /// The panel is open.
    pub open: bool,
    /// The starting squares of the followed pieces.
    pub selected: Vec<Square>,
    /// Their names.
    pub selected_names: Vec<String>,
    /// First move of the window.
    pub start_move: u16,
    /// Last move of the window.
    pub end_move: u16,
    /// Games or time.
    pub mode: TrackerMode,
    /// The colour scheme.
    pub scheme: TrackerScheme,
    /// The 64 squares, a1 to h8 (empty until the first count ends).
    pub cells: Vec<TrackerCell>,
    /// The squares by decreasing count (empty until the first count ends).
    pub ranking: Vec<TrackerRank>,
    /// Games in the filter the counts are for.
    pub games: u32,
    /// Games that were followed.
    pub games_used: u32,
    /// Games that could not be followed.
    pub games_skipped: u32,
    /// The counts are partial: the user stopped the count.
    pub interrupted: bool,
    /// The map is not for the filter, the window or the pieces now.
    pub stale: bool,
    /// `(done, total)` games of the count in flight.
    pub progress: Option<(u64, u64)>,
    /// There is no database.
    pub unavailable: bool,
    /// The last failure.
    pub issue: Option<PieceTrackerNotice>,
}

fn cells_and_ranking(
    result: &PieceTrackResult,
    scheme: TrackerScheme,
) -> (Vec<TrackerCell>, Vec<TrackerRank>) {
    let grid = result.grid();
    let cells = (0..64u32)
        .map(Square::new)
        .map(|square| {
            let index = square.to_usize();
            TrackerCell {
                square,
                count: result.counts.get(index).copied().unwrap_or(0),
                percent: result.percent(square),
                fraction: grid.squares.get(index).copied().unwrap_or(0.0),
                rgb: grid.rgb(scheme.engine(), index),
            }
        })
        .collect();
    let ranking = result
        .ranking()
        .into_iter()
        .enumerate()
        .map(|(i, r)| TrackerRank {
            position: i + 1,
            square: r.square,
            name: r.square.to_string(),
            count: r.count,
            percent: r.percent,
        })
        .collect();
    (cells, ranking)
}

/// Builds the view; the arguments are what the state holds.
pub(crate) fn build_view(
    open: bool,
    prefs: &PieceTrackerPrefs,
    result: Option<&PieceTrackResult>,
    stale: bool,
    progress: Option<(u64, u64)>,
    unavailable: bool,
    issue: Option<PieceTrackerNotice>,
) -> PieceTrackerView {
    let (cells, ranking) = result
        .map(|r| cells_and_ranking(r, prefs.scheme))
        .unwrap_or_default();
    PieceTrackerView {
        open,
        selected: prefs.squares.clone(),
        selected_names: prefs.squares.iter().map(ToString::to_string).collect(),
        start_move: prefs.start_move,
        end_move: prefs.end_move,
        mode: prefs.mode,
        scheme: prefs.scheme,
        cells,
        ranking,
        games: result.map_or(0, |r| r.games),
        games_used: result.map_or(0, |r| r.games_used),
        games_skipped: result.map_or(0, |r| r.games_skipped),
        interrupted: result.is_some_and(|r| r.interrupted),
        stale,
        progress,
        unavailable,
        issue,
    }
}
