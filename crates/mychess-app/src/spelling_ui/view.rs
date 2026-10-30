//! Snapshots of the windows of the spelling file, the name checker and "Add
//! Elo", ready to draw and free of `mychess-db` types.

use std::path::PathBuf;

use super::message::{PhotoCounts, Shared, SpellRow, SpellingCounts, SpellingNotice};
use crate::game_info::NameKind;

/// What the status line of the spelling file shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellingStatusView {
    /// A file is loaded.
    pub loaded: bool,
    /// The file.
    pub path: Option<PathBuf>,
    /// The counts of names.
    pub counts: Option<SpellingCounts>,
    /// The file is being read: bytes done and total.
    pub loading: Option<(u64, u64)>,
    /// The photos found.
    pub photos: Option<PhotoCounts>,
    /// The preference "Show photos".
    pub show_photos: bool,
}

/// One row of the list of corrections, with its place in the whole list
/// (the messages that edit a row name it by `index`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellRowView {
    /// The place in the whole list.
    pub index: usize,
    /// The row.
    pub row: SpellRow,
}

/// The name checker window.
#[derive(Debug, Clone, PartialEq)]
pub struct SpellView {
    /// The kind of name.
    pub kind: NameKind,
    /// The rows that pass the search box, in order.
    pub rows: Shared<Vec<SpellRowView>>,
    /// All the rows, the search box aside.
    pub total: usize,
    /// The rows ticked.
    pub ticked: usize,
    /// The text of the search box.
    pub find: String,
    /// "Surnames".
    pub surnames: bool,
    /// "Ambiguous".
    pub ambiguous: bool,
    /// The scan runs: games or names done and total.
    pub scanning: Option<(u64, u64)>,
    /// A scan ended (the list is not a leftover).
    pub scanned: bool,
    /// The scan was stopped; the list is partial.
    pub interrupted: bool,
    /// The confirmation of "Make corrections" is on screen.
    pub confirm: bool,
    /// The corrections are being written: done and total.
    pub writing: Option<(u64, u64)>,
    /// The database can be changed (not a PGN, not read-only).
    pub writable: bool,
    /// The database is the Clipbase (no confirmation, no file).
    pub in_clipbase: bool,
    /// How the last application ended.
    pub result: Option<SpellingNotice>,
}

/// The "Add Elo ratings" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct AddEloView {
    /// "Overwrite existing ratings?".
    pub overwrite: bool,
    /// "Add ratings to": the filter (`true`) or all games.
    pub only_filter: bool,
    /// The plan is being worked out: games done and total.
    pub planning: Option<(u64, u64)>,
    /// What the plan would change: ratings and games. Present when the
    /// dialog waits for the confirmation.
    pub planned: Option<(u32, u32)>,
    /// The ratings are being written: done and total.
    pub writing: Option<(u64, u64)>,
    /// The database can be changed.
    pub writable: bool,
    /// How the last run ended.
    pub result: Option<SpellingNotice>,
}

/// The flags of the name checker the keys and the timer read, without
/// building the whole [`SpellView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellFlags {
    /// A scan runs.
    pub scanning: bool,
    /// The confirmation of "Make corrections" is on screen.
    pub confirm: bool,
    /// The corrections are being written.
    pub writing: bool,
    /// The rows that pass the search box.
    pub rows: usize,
}

impl SpellFlags {
    /// A scan or a write runs (the windows redraw now and then).
    pub fn busy(self) -> bool {
        self.scanning || self.writing
    }
}

/// The flags of the "Add Elo" dialog the keys and the timer read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddEloFlags {
    /// The plan is being worked out.
    pub planning: bool,
    /// The plan waits for the confirmation.
    pub planned: bool,
    /// The ratings are being written.
    pub writing: bool,
    /// The last run ended with a result.
    pub has_result: bool,
}

impl AddEloFlags {
    /// The plan or the write runs.
    pub fn busy(self) -> bool {
        self.planning || self.writing
    }
}
