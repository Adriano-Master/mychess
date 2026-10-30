//! Opening and saving PGN files (spec M2-2, D124-D131).
//!
//! The functions here do blocking I/O and are meant for
//! [`perform`](crate::perform) (`spawn_blocking`) and for tests; `update`
//! never calls them.

mod collection;
mod names;
mod recent;
mod save;
mod scan;

use std::path::PathBuf;

pub use collection::{GameHeader, PgnCollection};
pub use names::{MAX_FILE_STEM_CHARS, ensure_pgn_extension, sanitize_file_name, suggest_file_name};
pub(crate) use recent::same_path;
pub use recent::{MAX_RECENT_FILES, RecentFiles};
pub use save::{SaveMode, save_pgn};
pub use scan::{LoadedGame, PgnLoad, PgnSource, ScanLimits, ScanOutcome, scan_pgn};

/// Where the game being edited came from; feeds the window title and
/// "Save" (D104).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GameOrigin {
    /// A new game, a position set up by hand, or a pasted FEN.
    #[default]
    New,
    /// Pasted PGN text.
    Clipboard,
    /// Game `index` (0-based) of `total` in a PGN file.
    PgnFile {
        /// The file (canonical path).
        path: PathBuf,
        /// Index of the game in the file.
        index: usize,
        /// Number of games in the file.
        total: usize,
        /// The file was only partly read: reading stopped at a limit or text
        /// outside the games was skipped. Saving over it would lose that
        /// part, so the controller asks first.
        partial: bool,
    },
    /// A game saved as a PGN file of its own.
    SavedFile {
        /// The file (canonical path).
        path: PathBuf,
    },
    /// The position of a record of an EPD file (spec M7-5, D3808): a game
    /// from it can be replaced without asking while it is unchanged.
    EpdRecord {
        /// The file of the EPD window.
        file: crate::epd_window::EpdFileId,
        /// The record.
        index: mychess_pgn::epd::EpdIndex,
    },
}

impl GameOrigin {
    /// The file behind the game, if there is one.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            GameOrigin::PgnFile { path, .. } | GameOrigin::SavedFile { path } => Some(path),
            GameOrigin::New | GameOrigin::Clipboard | GameOrigin::EpdRecord { .. } => None,
        }
    }
}
