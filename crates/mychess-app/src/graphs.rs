//! Score and time graphs of the game (M4-7): the pure part.
//!
//! Everything here is a function from a [`mychess_core::Game`] (plus a few
//! preferences) to plain geometry, so the GUI only has to draw it on a
//! `canvas` and can be tested without a window. The behaviour follows Scid's
//! `tcl/tools/graphs.tcl` and `sc_game scores`/`sc_game values`
//! (`src/tkscid.cpp`), see `docs/specs/m4-7-graficos-avaliacao-tempo.md`.
//!
//! - `series`: [`score_series`] and [`time_series`] read `[%eval]`,
//!   `[%emt]` and `[%clk]` from the comments of the **main line**;
//! - `axes`: [`Axis`], the y scales and the click-to-ply mapping;
//! - `line`: the continuous-line style (M4-9): [`squash`], the monotone curve,
//!   markers and [`nearest_ply`];
//! - `view`: [`GraphKind`], [`choose_kind`], [`score_graph_view`] and [`current_x`], which
//!   assemble a ready-to-draw [`ScoreGraphView`].
//!
//! The x coordinate of a point is `ply * 0.5 + 0.5`: 1.0 after White's first
//! move, 1.5 after Black's reply, 2.0 after White's second move.

// Graph coordinates are lossy by design: plies, seconds and tick indices
// become `f32` (exact up to 2^24 = 16.7 million, far above the number of
// moves a game can hold and above any useful axis; beyond it a coordinate is
// merely rounded), and clicks go back to integers after a clamp to
// `0..=plies`. Tick indices are `i64` computed from finite `f32` and
// capped by `MAX_TICKS`.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

mod axes;
mod line;
mod message;
mod prefs;
mod series;
mod state;
mod view;

pub use axes::{Axis, MAX_TICKS, ply_at_x, time_step, x_of_ply};
pub use line::{
    CurveSegment, GRID_EVALS, GraphStyle, LineGraphView, LinePoint, Marker, MarkerKind, SQUASH_K,
    grid_lines, line_graph_view, line_view_from_scores, monotone_segments, move_ticks, nearest_ply,
    squash,
};
pub use message::GraphMessage;
pub use series::{
    Invert, SCORE_LIMIT, ScorePoint, TimePoint, TimeSeries, score_series, time_series,
};
pub use state::GraphState;
pub(crate) use state::{BarKey, ViewKey};
pub use view::{
    GraphKind, GraphPrefs, ScoreGraphView, choose_kind, current_x, graph_title, report_graph_view,
    score_graph_view,
};

#[cfg(test)]
mod test_util;
