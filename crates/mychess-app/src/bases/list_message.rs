//! Messages of the game list actions (spec M3-4, slice 4c; D556 onwards).

use std::path::PathBuf;
use std::sync::Arc;

use mychess_db::Filter;

use super::SlotId;
use super::columns::ListColumn;
use super::error::BaseError;
use super::open::FileStamp;
use super::source::SharedSource;
use crate::error::RequestId;

/// How a click changes the selection (the modifiers the front-end saw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    /// A plain click: only this game is selected.
    Only,
    /// Ctrl/Cmd+click: this game is added or removed.
    Toggle,
    /// Shift+click: every game from the anchor to this one is selected.
    Range,
}

/// What a quick search does with its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    /// "Find": move to the next game that contains the text.
    Find,
    /// "Filter": remove from the filter the games that do not contain it;
    /// terms separated by `+` all have to match.
    Filter,
}

/// A quick search to run (the payload of the effect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSpec {
    /// Find or filter.
    pub kind: SearchKind,
    /// The text; for a filter, terms separated by `+`.
    pub text: String,
    /// Whether case is ignored.
    pub ignore_case: bool,
    /// Find: the position in the database order after which to look (0 =
    /// from the start). Unused when `tail` is set.
    pub from: u32,
    /// Find in a sorted view: the games of the view after the current one,
    /// in view order.
    pub tail: Option<Vec<u32>>,
}

/// The result of a quick search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchOutcome {
    /// The next game found, if any (its number).
    Found(Option<u32>),
    /// The filter after the terms were applied.
    Filtered {
        /// The new filter of the slot.
        filter: Box<Filter>,
        /// How many games were removed.
        removed: u32,
        /// How many index entries could not be read (they were removed).
        unreadable: u64,
    },
}

/// What a list job in flight is doing (for the progress shown to the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListJobKind {
    /// "Find".
    Find,
    /// "Filter".
    Filter,
    /// Sorting by a column.
    Sort(ListColumn),
    /// Copying games to the Clipbase.
    Copy,
    /// Copying the filter (or the selection) of one database to another
    /// (slice 4d).
    CopyGames,
    /// Changing flags in the file.
    Flags,
    /// Classifying the games by ECO code, or counting what would change
    /// (spec M5-7b).
    EcoClassify,
    /// Looking for the novelty of the game in the reference database
    /// (spec M5-7b).
    EcoNovelty,
    /// Importing PGN files (spec M3-8).
    ImportPgn,
    /// Exporting games to a PGN file (spec M3-9).
    ExportGames,
    /// Writing the corrected names into the file (spec M6-3, D3462). Like
    /// [`ListJobKind::Flags`], it rewrites the file of its slot.
    SpellApply,
    /// Writing the ratings of the spelling file into the games (spec M6-3,
    /// D3462). Like [`ListJobKind::Flags`], it rewrites the file of its slot.
    EloAdd,
}

/// A database whose flags were changed and reopened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagOutcome {
    /// The database reopened after the change; `None` if it could not be
    /// opened again (`error` says why and `changed` is still true).
    pub source: Option<SharedSource>,
    /// How many games changed.
    pub changed: u32,
    /// Why the change stopped early, if it did (D573): the games that were
    /// written stay written, and the database was reopened anyway.
    pub error: Option<BaseError>,
    /// The state of the file after the change.
    pub stamp: Option<FileStamp>,
}

/// Which games of the source a copy between databases takes (slice 4d).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyScope {
    /// Every game of the filter of the source (dragging a tab).
    Filter,
    /// The selected games of the source list (dragging rows).
    Selection,
}

/// The games a copy job takes, in the order of the source database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyList {
    /// A copy of the filter of the source: its games in ascending order.
    Filter(Box<Filter>),
    /// These games, ascending.
    Numbers(Vec<u32>),
}

impl CopyList {
    /// How many games the job will copy.
    pub fn count(&self) -> u32 {
        match self {
            CopyList::Filter(filter) => filter.count(),
            CopyList::Numbers(numbers) => u32::try_from(numbers.len()).unwrap_or(u32::MAX),
        }
    }
}

/// Where a copy job writes (slice 4d, D577).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyTarget {
    /// The Clipbase: in memory, appended batch by batch.
    Clipbase(SharedSource),
    /// An editable `.si4` file, written by a short-lived writer.
    Si4 {
        /// The `.si4` file.
        path: PathBuf,
        /// The number of games the slot was loaded with.
        expected_total: u32,
        /// The state of the file when the slot was loaded (D573).
        stamp: Option<FileStamp>,
    },
}

/// A database opened again after a copy wrote to its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reloaded {
    /// The new database.
    pub source: SharedSource,
    /// The state of the file after the copy.
    pub stamp: Option<FileStamp>,
}

/// What a copy job did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyOutcome {
    /// Games written to the target. They stay written even when `error`
    /// is set: the copy stops at the first problem (D578).
    pub copied: u32,
    /// Why the copy stopped before the end, if it did; `Cancelled` when
    /// the user stopped it.
    pub error: Option<BaseError>,
    /// Games written with a name shortened to the 255 bytes the format
    /// keeps (D590).
    pub truncated: u32,
    /// The target database read again (an `.si4` target; `None` for the
    /// Clipbase, which was changed in place).
    pub reloaded: Option<Reloaded>,
}

/// One end of a copy, named for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyEnd {
    /// The slot.
    pub slot: SlotId,
    /// The file name; empty for the Clipbase and for a slot that is not
    /// open (the front-end writes the Clipbase's name or the slot number).
    pub name: String,
}

/// Why a copy between databases was refused before it started (the Scid
/// checks in `copyFilter`, plus the case of the selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyRefusal {
    /// The source and the target are the same database.
    SameBase,
    /// The source is not open.
    SourceNotOpen,
    /// The target is not open.
    TargetNotOpen,
    /// The target cannot be written (a PGN file, a read-only database or
    /// an `.si4` without write permission).
    TargetReadOnly,
    /// The filter of the source has no games.
    EmptyFilter,
    /// No game of the source list is selected.
    NothingSelected,
}

/// The question "copy N games from X to Y?" (D580).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyPrompt {
    /// The source.
    pub from: CopyEnd,
    /// The target.
    pub to: CopyEnd,
    /// Which games.
    pub scope: CopyScope,
    /// How many games there are to copy now.
    pub count: u32,
}

/// An action on the list of a slot.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ListMessage {
    /// A click on a game (the front-end tells which modifiers were down).
    Pick {
        /// The slot.
        slot: SlotId,
        /// The game number.
        number: u32,
        /// How the selection changes.
        mode: PickMode,
    },
    /// Replace the selection with these games (those not in the filter are
    /// left out): Ctrl/Cmd+A sends the visible rows.
    SelectNumbers {
        /// The slot.
        slot: SlotId,
        /// The games.
        numbers: Vec<u32>,
    },
    /// Remember "Ignore case" of the quick search.
    SetIgnoreCase {
        /// The slot.
        slot: SlotId,
        /// Whether case is ignored.
        ignore_case: bool,
    },
    /// "Find": the next game that contains the search text, after the
    /// selected one.
    Find {
        /// The slot.
        slot: SlotId,
    },
    /// "Filter": keep in the filter only the games that contain the search
    /// text.
    FilterByText {
        /// The slot.
        slot: SlotId,
    },
    /// Stop the search, sort or copy in flight.
    CancelJob,
    /// "Reset": every game back in the filter.
    ResetFilter {
        /// The slot.
        slot: SlotId,
    },
    /// "Negate": the games out come in, the games in go out.
    NegateFilter {
        /// The slot.
        slot: SlotId,
    },
    /// Remove the selected games from the filter (Delete).
    RemoveSelected {
        /// The slot.
        slot: SlotId,
    },
    /// Remove from the filter every game above `number` in the view.
    RemoveAbove {
        /// The slot.
        slot: SlotId,
        /// The game.
        number: u32,
    },
    /// Remove from the filter every game below `number` in the view.
    RemoveBelow {
        /// The slot.
        slot: SlotId,
        /// The game.
        number: u32,
    },
    /// "Set filter": only the selected games stay in the filter.
    KeepSelected {
        /// The slot.
        slot: SlotId,
    },
    /// Sort the view by a column; the same column again flips the
    /// direction.
    SortBy {
        /// The slot.
        slot: SlotId,
        /// The column.
        column: ListColumn,
    },
    /// Copy the selected games to the end of the Clipbase.
    CopySelected {
        /// The slot.
        slot: SlotId,
    },
    /// Toggle the "deleted" flag of the selected games (Ctrl+Delete).
    ToggleDeleted {
        /// The slot.
        slot: SlotId,
    },
    /// Copy the filter (or the selection) of a database to the end of
    /// another (dragging a tab onto a tab, slice 4d). Asks first when the
    /// "confirm copy" preference is on and the target is not the Clipbase.
    CopyGames {
        /// The source slot.
        from: SlotId,
        /// The target slot.
        to: SlotId,
        /// Which games of the source.
        scope: CopyScope,
    },
    /// The answer to the question "copy N games?".
    AnswerCopy(bool),
    /// Mark a database read-only, or editable again (session only).
    SetReadOnly {
        /// The slot.
        slot: SlotId,
        /// The new state.
        read_only: bool,
    },

    // ---- answers to effects ----
    /// A quick search finished.
    Searched {
        /// The request.
        request: RequestId,
        /// What was asked.
        kind: SearchKind,
        /// The result.
        result: Result<SearchOutcome, BaseError>,
    },
    /// A sort finished: all the games, smallest first.
    Sorted {
        /// The request.
        request: RequestId,
        /// The result.
        result: Result<Arc<Vec<u32>>, BaseError>,
    },
    /// A copy between databases ended (or stopped).
    GamesCopied {
        /// The request.
        request: RequestId,
        /// What was done, or why nothing was.
        result: Result<CopyOutcome, BaseError>,
    },
    /// Flags were changed in the file and the database was reopened.
    Flagged {
        /// The request.
        request: RequestId,
        /// The result.
        result: Result<FlagOutcome, BaseError>,
    },
    /// The ECO classification ended (spec M5-7b).
    EcoClassified {
        /// The request.
        request: RequestId,
        /// The result.
        result: Result<crate::eco_ui::ClassifyOutcome, BaseError>,
    },
    /// The novelty search ended (spec M5-7b).
    EcoNovelty {
        /// The request.
        request: RequestId,
        /// The result.
        result: Result<crate::eco_ui::NoveltyOutcome, BaseError>,
    },
    /// The import of PGN files ended (spec M3-8).
    PgnImported {
        /// The request.
        request: RequestId,
        /// What was done, or why nothing was.
        result: Result<Box<crate::file_import::FileImportOutcome>, BaseError>,
    },
    /// The export of games ended (spec M3-9).
    GamesExported {
        /// The request.
        request: RequestId,
        /// What was done, or why nothing was.
        result: Result<Box<crate::games_export::GamesExportOutcome>, BaseError>,
    },
}
