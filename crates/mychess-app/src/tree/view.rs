//! What the front-end draws: numbers only (spec M5-2, D968, D969).
//!
//! The text of the Scid's tree window (`%5.1f%%`, column alignment) is the
//! front-end's job; it can use the rounding helpers already applied here:
//! shares and scores are `(percent, tenths)` pairs, truncated as in the
//! Scid.

use mychess_core::Color;

use super::message::TreeIssue;
use super::prefs::{TreeFastMode, TreeSort};

/// The width of the bar of a row, in logical units (Scid: a 60 pixel canvas
/// plus 1 for the border).
pub const BAR_WIDTH: f32 = 61.0;

/// The tricolour bar of a row: white from `0` to `white_end`, black from
/// `black_start` to [`BAR_WIDTH`], the background between (D969).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSegments {
    /// End of the white part; `None` when it is not drawn.
    pub white_end: Option<f32>,
    /// Start of the black part; `None` when it is not drawn.
    pub black_start: Option<f32>,
}

/// The bar for a score and a draw percentage (Scid: `tree.tcl`,
/// `displayLines` and `createCanvas`).
///
/// `score_permille` is the expected score of White per thousand and
/// `draw_percent` the whole percentage of draws. With `success` the score in
/// percent, the white part ends at `(success - draw/2) * 0.6 + 1` and is
/// drawn only above `0.1`; the black part starts at
/// `(success + draw/2) * 0.6 + 1` and is drawn only below `60.9`.
///
/// ```
/// let bar = mychess_app::bar_segments(1000, 0);
/// assert_eq!(bar.black_start, None);
/// ```
pub fn bar_segments(score_permille: u32, draw_percent: u32) -> BarSegments {
    // Both inputs are small (a permille and a percentage), exactly
    // representable in f64.
    let success = f64::from(score_permille) / 10.0;
    let draw = f64::from(draw_percent);
    let won = (success - draw / 2.0) * 0.6 + 1.0;
    let lost = (success + draw / 2.0) * 0.6 + 1.0;
    // The narrowing to f32 only loses precision far below a pixel.
    #[allow(clippy::cast_possible_truncation)]
    let (won, lost) = (won as f32, lost as f32);
    BarSegments {
        white_end: (won > 0.1).then_some(won),
        black_start: (lost < 60.9).then_some(lost),
    }
}

/// A row of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeRowView {
    /// The index to send in
    /// [`TreeMessage::RowClicked`](super::TreeMessage::RowClicked).
    pub index: usize,
    /// The move in SAN, or `[end]`.
    pub san: String,
    /// Training: draw no move text and no ECO code.
    pub hidden: bool,
    /// The `[end]` row (games that end here).
    pub is_end: bool,
    /// A click does something now.
    pub clickable: bool,
    /// The move is the next one of the main line of the game (D966).
    pub is_next_move: bool,
    /// Number of games.
    pub games: u32,
    /// Share of the games as `(percent, tenths)`.
    pub share: (u32, u32),
    /// Games by result: White, Black, draw, no result.
    pub results: [u32; 4],
    /// Expected score for White as `(percent, tenths)`.
    pub score: (u32, u32),
    /// Draws, whole percent.
    pub draw_percent: u32,
    /// Average rating of the side that moves.
    pub avg_elo: Option<u32>,
    /// Performance rating.
    pub perf: Option<u32>,
    /// Average year.
    pub avg_year: Option<u32>,
    /// ECO code of the position after the move.
    pub eco: Option<String>,
    /// The bar.
    pub bar: BarSegments,
    /// What the open mask says about the move; `None` without a mask, for
    /// the `[end]` row and for the rows the training hides (spec M5-4b,
    /// D1827).
    pub mask: Option<super::MaskCell>,
}

/// The `TOTAL` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeTotalView {
    /// Number of games.
    pub games: u32,
    /// Games by result: White, Black, draw, no result.
    pub results: [u32; 4],
    /// Expected score for White as `(percent, tenths)`.
    pub score: (u32, u32),
    /// Draws, whole percent.
    pub draw_percent: u32,
    /// Average rating.
    pub avg_elo: Option<u32>,
    /// Performance rating.
    pub perf: Option<u32>,
    /// Average year.
    pub avg_year: Option<u32>,
}

/// What goes where the table would be.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeBody {
    /// The panel is closed.
    Closed,
    /// The base has no games or is not usable: "base not available".
    Unavailable,
    /// The first search is running.
    Loading,
    /// The table.
    Rows,
    /// A search worked and no game reaches the position.
    Empty,
    /// The search failed.
    Issue(TreeIssue),
}

/// The panel, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeView {
    /// What to show.
    pub body: TreeBody,
    /// The rows (empty unless `body` is [`TreeBody::Rows`]).
    pub rows: Vec<TreeRowView>,
    /// The totals line; `None` when there are no rows.
    pub total: Option<TreeTotalView>,
    /// The side to move in the position of the rows.
    pub turn: Color,
    /// Short display: hide rating, performance, year and ECO.
    pub short: bool,
    /// The order of the rows.
    pub sort: TreeSort,
    /// The search mode.
    pub fast_mode: TreeFastMode,
    /// "Refresh" is on.
    pub autorefresh: bool,
    /// "Adjust filter" is on.
    pub adjust: bool,
    /// A search is running: show the activity indicator and grey the table.
    pub searching: bool,
    /// Games that could not be read in the last search.
    pub unreadable: u64,
    /// The last search was served from the cache.
    pub from_cache: bool,
    /// Games whose record the last search read.
    pub searched: u32,
    /// Time the last search took, in milliseconds.
    pub elapsed_ms: u64,
    /// Training is on (the moves are hidden).
    pub training: bool,
    /// Training can be turned on now.
    pub training_available: bool,
    /// The tree is locked on a database.
    pub locked: bool,
    /// The name of the database of the tree (the file name without its
    /// extension); `None` for the Clipbase or when there is none.
    pub target_name: Option<String>,
    /// The target is the Clipbase.
    pub target_is_clipbase: bool,
    /// The best games section; `None` when closed.
    pub best: Option<super::BestView>,
    /// The cache of the trees: size, auto-save, job and last result
    /// (spec M5-3b).
    pub cache: super::CacheView,
    /// The open mask; `None` without one (spec M5-4b, D1827).
    pub mask: Option<super::MaskView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_white_wins_is_a_full_white_bar() {
        let bar = bar_segments(1000, 0);
        assert_eq!(bar.white_end, Some(61.0));
        assert_eq!(bar.black_start, None);
    }

    #[test]
    fn no_white_wins_leaves_the_border_white_and_the_rest_black() {
        let bar = bar_segments(0, 0);
        assert_eq!(bar.white_end, Some(1.0));
        assert_eq!(bar.black_start, Some(1.0));
    }

    #[test]
    fn all_draws_is_grey_between_the_borders() {
        let bar = bar_segments(500, 100);
        assert_eq!(bar.white_end, Some(1.0));
        assert_eq!(bar.black_start, None);
    }

    #[test]
    fn an_intermediate_row_follows_the_formula() {
        // 54.7 % with 37 % draws: 0.6 * (54.7 -+ 18.5) + 1.
        let bar = bar_segments(547, 37);
        let white = bar.white_end.expect("white");
        let black = bar.black_start.expect("black");
        assert!((white - 22.72).abs() < 1e-3, "{white}");
        assert!((black - 44.92).abs() < 1e-3, "{black}");
    }
}
