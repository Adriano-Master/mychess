//! The tablebase panel and the endgame training (spec M8-8, D2550-D2599,
//! ADR 0012).
//!
//! The panel shows the result of every legal move of the position on the
//! board, from local Syzygy files or from the Lichess online tablebase. The
//! training lets the user play an endgame against the tablebase.
//!
//! - `prefs`: [`TablebasePrefs`] (`[tablebase]` in `preferences.toml`);
//! - `message`: [`TablebaseMessage`], [`TablebaseEffect`] (the `perform` of
//!   the probes), the shared handles and the notices;
//! - `training`: the pure rules of the training (grades, slips, verdict);
//! - `view`: [`TablebaseView`], the ready-to-draw snapshot for the front-end,
//!   which never sees `mychess-tablebase`.
//!
//! The flow itself (refresh at the end of `update`, answers, watchdog,
//! training) is in `state/tablebase_flow.rs`.

mod message;
mod prefs;
mod training;
mod view;

pub(crate) use message::perform;
pub use message::{
    DelayKind, LocalHandle, OnlineHandle, OpenedSet, TablebaseEffect, TablebaseMessage,
    TablebaseNotice,
};
pub use prefs::{MAX_DIRECTORIES, MAX_PATH_CHARS, TablebasePrefs};
pub use training::{EndReason, Grade, TrainingOutcome, Verdict, grade_of, verdict};
pub(crate) use training::{Training, grade_of_end};
pub(crate) use view::issue_of;
pub use view::{
    DirectoryView, IssueView, LocalSummary, RowView, TablebaseStatus, TablebaseView, TrainingView,
};

pub use mychess_tablebase::{Category as TbCategory, Source as TbSource};

/// The materials offered for a new training position (D2577).
pub const TRAINING_MATERIALS: [&str; 10] = [
    "KQvK", "KRvK", "KPvK", "KBNvK", "KRvKP", "KQvKR", "KRvKB", "KRvKN", "KQvKP", "KRPvKR",
];

/// Milliseconds the online query waits for the position to stay still (D2567).
pub const DEBOUNCE_MS: u32 = 300;
/// Milliseconds after which a local query is abandoned (D2563).
pub const LOCAL_WATCHDOG_MS: u32 = 15_000;
/// Milliseconds after which an online query is abandoned (D2563).
pub const ONLINE_WATCHDOG_MS: u32 = 10_000;
/// Milliseconds after which the reading of the folders is abandoned (a
/// folder on a dead network share must not leave the panel loading for ever).
pub const OPEN_WATCHDOG_MS: u32 = 15_000;
/// Milliseconds between the move of the user and the answer of the training
/// (D2574).
pub const REPLY_DELAY_MS: u32 = 250;
