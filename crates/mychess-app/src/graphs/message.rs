//! The messages of the graph (spec M4-7).

use super::line::GraphStyle;
use super::view::GraphKind;

/// An event of the score and time graph.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum GraphMessage {
    /// Opens or closes the graph, for a front-end **without a layout**
    /// (headless use). A front-end that owns the layout (the GUI) is the
    /// source of truth and reports it with [`GraphMessage::SetOpen`]; a
    /// `Toggle` there is overwritten by the next `SetOpen`. The action
    /// `ToggleGraphPane` does not send it.
    Toggle,
    /// The graph panel is open (`true`) or closed. The view is only built
    /// while it is open.
    SetOpen(bool),
    /// Chooses what the graph shows (Auto, Score, Time and the combos).
    SetKind(GraphKind),
    /// Inverts the scores of the comments after the moves of Black ("Invert
    /// White" of the Scid: the side to move in the evaluated position).
    ToggleInvertWhite,
    /// Inverts the scores of the comments after the moves of White ("Invert
    /// Black" of the Scid).
    ToggleInvertBlack,
    /// Chooses line or bars for the scores (M4-9).
    SetStyle(GraphStyle),
    /// Shows or hides the bar of the current move.
    ToggleHighlight,
    /// A click on the graph at the x coordinate `x` (graph units): goes to
    /// the move there. A click outside the axis is ignored.
    Click(f32),
    /// Goes to the main-line ply `ply` (clamped to the plies of the view),
    /// without going through the axis of the bars. The style Line sends it:
    /// a ply without a score stays clickable and ply 0 is reachable (D1556).
    GoToPly(usize),
}
