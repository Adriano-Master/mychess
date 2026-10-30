//! Calculation of variants, the Stoyko exercise (spec M8-6, D2300-D2349).
//!
//! The user is shown a position and calculates **in the head** the lines he
//! finds (candidate moves, answers, the end of the line), entering them as
//! text without moving the pieces and ending each with his evaluation. Then
//! an engine analyses the position and the lines, and the program compares:
//! which good moves he found or missed, how deep and how right each line is,
//! and a score. Scid: `tcl/tools/calvar.tcl`.
//!
//! - `config`: what the dialog edits;
//! - `line`: the lines of the user, their text and their evaluation symbols;
//! - `eval`: the evaluations of the engine and their classes;
//! - `compare`: the comparison and the score (pure);
//! - `store`: the history and `calvar.toml`;
//! - `engine`: the jobs of the engine (two stages);
//! - `machine`: the pure state machine ([`Calvar`]);
//! - `message` and `effect`: [`CalvarMessage`] and the effects;
//! - `flow`: the glue with [`AppState`](crate::AppState).

mod compare;
mod config;
mod effect;
mod engine;
mod eval;
mod flow;
mod line;
mod machine;
mod message;
mod pick;
mod review;
mod store;

pub use compare::{
    ACCURACY_POINTS, CANDIDATES_POINTS, CRUCIAL_PENALTY, DECISIVE_CP, FULL_DEPTH_PLIES,
    GOOD_MARGIN_CP, GoodMove, LineReport, QUALITY_POINTS, Report, SOUND_TOLERANCE_CP, ScoreParts,
    compare, good_moves, score_of,
};
pub use config::{
    CalvarConfig, LINE_MS_RANGE, LINES_RANGE, MAX_FEN_CHARS, MULTIPV_RANGE, PositionSource,
    ROOT_MS_RANGE, TIME_LIMIT_RANGE,
};
pub(crate) use effect::perform;
pub use engine::{CalvarEngine, EngineFailure, LinesJob, RootJob, analyze_lines, analyze_root};
pub use eval::{Eval, LineProbes, MATE_CP, Probe, RootLine};
pub(crate) use flow::CalvarFlow;
pub use line::{
    Assessment, MAX_ENTRY_CHARS, MAX_LINE_PLIES, MAX_LINES, ReadMoves, UserLine, read_moves,
    san_line,
};
pub use machine::{
    Calvar, CalvarFailure, CalvarOrigin, CalvarPhase, CalvarView, GAME_WATCHDOG_MS, GameWait,
    HistoryRow, LineView, MAX_TICK_MS, Out, REVIEW_ROWS, WATCHDOG_MS,
};
pub use message::{CalvarEffect, CalvarMessage, CalvarNotice, CalvarStartRequest};
pub use pick::{DEFAULT_PLY, game_position};
pub use review::{GoodRecord, LineRecord, MAX_RECORD_GOOD, MAX_RECORD_PV, ReviewRecord};
pub use store::{
    CalvarStats, CalvarStore, HistoryEntry, LoadedStore, MAX_HISTORY, STORE_FILE, STORE_VERSION,
    load_file, parse_store, render_store, save_file, store_path,
};
