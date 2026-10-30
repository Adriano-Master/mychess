//! Events of the evaluation bar, the arrows and the traffic lights (spec
//! M4-5). The scale, the mapping and the other options are changed with
//! [`PrefChange`](crate::PrefChange).

use mychess_core::Square;

use crate::engines::RunId;

/// A command of the board overlays of the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EvalBarMessage {
    /// Shows or hides the evaluation bar (`Visualizar > Barra de avaliacao`).
    ToggleBar,
    /// Shows or hides the traffic lights.
    ToggleTrafficLights,
    /// Numbers the secondary arrows, or not (Scid `numberPVArrows`).
    ToggleArrowNumbers,
    /// The button "arrows on the board" of an execution: the engine of `run`
    /// draws, or stops drawing if it already did (Scid `setBoardEngine`).
    ToggleArrows(RunId),
    /// Switches between the arrows and the sequence (Scid `nextBoardType`).
    CycleArrowMode,
    /// Next submode of the sequence: both sides, White, Black.
    CycleSequenceSide,
    /// A click on the tip of an arrow: plays the first move of line `line`
    /// of `run` (D457), as "Add move" does for line 1. The squares are those
    /// of the arrow that was clicked: the click is refused if that arrow is
    /// not on the board any more (D826).
    PlayArrow {
        /// The execution that drew the arrow.
        run: RunId,
        /// The MultiPV line, from 1.
        line: u8,
        /// Where the arrow starts.
        from: Square,
        /// Where the arrow ends.
        to: Square,
    },
}
