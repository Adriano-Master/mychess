//! Endgame tablebases for MyChess (milestone M8-8, spec
//! `docs/specs/m8-8-tablebases.md`, ADR 0012).
//!
//! Layer: Model. This crate depends on `mychess-core`, on `mychess-online`
//! (its HTTP client, so there is one TLS stack and one request gate), on
//! `shakmaty` and on `shakmaty-syzygy`. It knows no `iced` and no app types;
//! `mychess-app` wraps its values in messages and runs the probes as effects.
//!
//! Contents:
//! * [`category`]: [`Category`] (the 8 results of the Lichess API),
//!   [`Evaluation`], [`MoveEntry`], [`PositionReport`] and the order of the
//!   list of moves;
//! * [`probe`]: the traits [`PositionProbe`] (local, blocking) and
//!   [`OnlineProbe`] (network, `async`), and the checks both share;
//! * [`local`]: [`LocalTablebase`], Syzygy files read lazily from some folders
//!   (WDL and DTZ; there is **no distance to mate** in Syzygy);
//! * [`online`] and [`cache`]: the Lichess tablebase and its cache;
//! * [`generate`]: random positions of a material, for the training;
//! * `testing` (feature `testing`): stub sources for tests.
//!
//! Rules: external input (table files, folders, HTTP answers) never panics and
//! never allocates in proportion to a number it carries; the crate has no
//! `unsafe` code and does not use the `mmap` feature of `shakmaty-syzygy`; no
//! test touches the real network, and the tests with real tables need the
//! folder in `MYCHESS_TEST_SYZYGY`.

pub mod cache;
pub mod category;
mod convert;
pub mod error;
pub mod generate;
pub mod local;
pub mod online;
pub mod probe;
#[cfg(feature = "testing")]
pub mod testing;

pub use cache::{CachedOnline, ProbeCache};
pub use category::{
    Category, Counts, Evaluation, MoveEntry, PositionReport, Source, compare_moves,
};
pub use error::{Result, TablebaseError};
pub use local::{DirReport, DirStatus, FileIssue, FileIssueKind, LocalTablebase, OpenReport};
pub use online::LichessTablebase;
pub use probe::{
    BoxFuture, ONLINE_MAX_PIECES, OnlineProbe, PositionProbe, check_applicable, material_name,
    piece_count, position_key,
};
