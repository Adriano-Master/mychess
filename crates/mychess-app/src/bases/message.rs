//! Messages and notices of the slots (spec M3-4).

use std::path::PathBuf;

use super::SlotId;
use super::browser::BrowseMessage;
use super::error::BaseError;
use super::list_message::{CopyEnd, CopyRefusal, ListMessage};
use super::open::OpenOutcome;
use crate::error::RequestId;
use crate::shared::SharedGame;

/// An event of the slots and the Clipbase.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BasesMessage {
    /// Open a database file in the first free slot. A file that is already
    /// open only activates its slot.
    Open(PathBuf),
    /// Stop the load that is in flight.
    CancelOpen,
    /// The time a quit waits for the list job to wind down ran out.
    QuitDeadline,
    /// Make a slot the current one (Ctrl-1 to Ctrl-9). An empty slot is
    /// ignored.
    Select(SlotId),
    /// The next open slot, in order, wrapping around (Ctrl-Tab).
    Next,
    /// The previous open slot, in order, wrapping around (Shift-Tab).
    Previous,
    /// Close a slot, asking first if its game was altered. The Clipbase is
    /// not closed: it is cleared.
    Close(SlotId),
    /// Load game `number` of a slot into the editor, activating the slot
    /// first and asking before an altered game is replaced.
    LoadGame {
        /// The slot.
        slot: SlotId,
        /// The game number, from 1.
        number: u32,
    },
    /// Copy the game being edited to the end of the Clipbase.
    ClipbaseCopy,
    /// Replace the game being edited with the active game of the Clipbase.
    ClipbasePaste,
    /// Remove every game of the Clipbase.
    ClipbaseClear,
    /// Scroll the list of a slot so its `first`-th game of the filter (from
    /// 1) is the first row. Clamped to the filter.
    SetListFirst {
        /// The slot.
        slot: SlotId,
        /// The position in the filter, from 1.
        first: u32,
    },
    /// Select a game in the list of a slot (`None` clears the selection). A
    /// game that is not in the filter is ignored.
    SelectRow {
        /// The slot.
        slot: SlotId,
        /// The game number.
        number: Option<u32>,
    },
    /// Remember the text of the quick-search field of a slot.
    SetSearchText {
        /// The slot.
        slot: SlotId,
        /// The text.
        text: String,
    },

    /// An action on the list of a slot (slice 4c).
    List(ListMessage),

    /// An event of the game browser (slice 4e).
    Browse(BrowseMessage),

    // ---- answers to effects ----
    /// A database was opened (answer to [`Effect::OpenBase`](crate::Effect)).
    Opened {
        /// The request.
        request: RequestId,
        /// The outcome, or why it failed.
        result: Result<OpenOutcome, BaseError>,
    },
    /// A game was read (answer to [`Effect::LoadBaseGame`](crate::Effect)).
    GameLoaded {
        /// The request.
        request: RequestId,
        /// The game, or why it could not be read.
        result: Result<SharedGame, BaseError>,
    },
}

/// A remark about the slots for the status bar or a message box.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BasesNotice {
    /// "Too many databases are open; close one first."
    NoFreeSlot,
    /// A file is still being opened; wait or cancel it.
    OpenInProgress,
    /// A database could not be opened or read.
    Failed(BaseError),
    /// "You are already in the clipbase database."
    AlreadyInClipbase,
    /// There is no game in the Clipbase to paste.
    ClipbaseEmpty,
    /// A database was opened (or was already open) while a question about
    /// the game was open: it is in its slot but was not made current;
    /// select it once the question is answered.
    NotActivated,
    /// A game was read for the editor but a question about the current
    /// game was open, so it was not loaded; ask again.
    GameNotLoaded,
    /// A search, sort or copy of the list is still running; wait or cancel
    /// it.
    ListBusy,
    /// "Find" reached the end without a hit; the list went back to the
    /// start.
    NotFound,
    /// A text filter was applied: how many games it removed.
    Filtered {
        /// Games removed from the filter.
        removed: u32,
    },
    /// Games were copied to the Clipbase.
    Copied {
        /// How many.
        count: u32,
    },
    /// A copy between databases was refused before it started (slice 4d).
    CopyRefused {
        /// Why.
        reason: CopyRefusal,
        /// The source.
        from: CopyEnd,
        /// The target.
        to: CopyEnd,
    },
    /// The games of one database were copied to another.
    FilterCopied {
        /// How many games were written.
        count: u32,
        /// The source.
        from: CopyEnd,
        /// The target.
        to: CopyEnd,
    },
    /// Like [`BasesNotice::FilterCopied`], with something to add: names
    /// that were shortened to fit the format, or a list that was reset
    /// because the file had another number of games than expected.
    FilterCopiedNotes {
        /// How many games were written.
        count: u32,
        /// The source.
        from: CopyEnd,
        /// The target.
        to: CopyEnd,
        /// Games with a name shortened to 255 bytes.
        truncated: u32,
        /// The target's filter, selection and sort were reset.
        reset: bool,
    },
    /// A copy between databases stopped before the end (an error, or the
    /// user cancelled): the games written stay.
    CopyStopped {
        /// How many games were written.
        count: u32,
        /// Why it stopped; `Cancelled` for the user.
        error: BaseError,
        /// The source.
        from: CopyEnd,
        /// The target.
        to: CopyEnd,
    },
    /// The action needs a database that can be changed (not a PGN file, the
    /// Clipbase or one marked read-only).
    ReadOnly,
    /// The action needs games selected.
    NothingSelected,
    /// The result of a search or sort was dropped because the database
    /// changed while it ran.
    Stale,
    /// The database was reloaded with another number of games: the filter,
    /// selection and sort were reset.
    ListReset,
    /// Some index entries could not be read during a text filter; those
    /// games were removed.
    UnreadableEntries {
        /// How many.
        count: u64,
    },
    /// The browser was asked for more games than it shows (slice 4e).
    BrowserLimit {
        /// The games shown.
        shown: u32,
        /// The games asked for.
        asked: u32,
    },
}

impl BasesNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            BasesNotice::NoFreeSlot => "base-notice-no-free-slot",
            BasesNotice::OpenInProgress => "base-notice-open-in-progress",
            BasesNotice::Failed(error) => error.message_key(),
            BasesNotice::AlreadyInClipbase => "base-notice-already-in-clipbase",
            BasesNotice::ClipbaseEmpty => "base-notice-clipbase-empty",
            BasesNotice::NotActivated => "base-notice-not-activated",
            BasesNotice::GameNotLoaded => "base-notice-game-not-loaded",
            BasesNotice::ListBusy => "base-notice-list-busy",
            BasesNotice::NotFound => "base-notice-not-found",
            BasesNotice::Filtered { .. } => "base-notice-filtered",
            BasesNotice::Copied { .. } => "base-notice-copied",
            BasesNotice::CopyRefused { reason, .. } => match reason {
                CopyRefusal::SameBase => "base-notice-copy-same",
                CopyRefusal::SourceNotOpen => "base-notice-copy-source-closed",
                CopyRefusal::TargetNotOpen => "base-notice-copy-target-closed",
                CopyRefusal::TargetReadOnly => "base-notice-copy-target-read-only",
                CopyRefusal::EmptyFilter => "base-notice-copy-empty-filter",
                CopyRefusal::NothingSelected => "base-notice-copy-nothing-selected",
            },
            BasesNotice::FilterCopied { .. } => "base-notice-filter-copied",
            BasesNotice::FilterCopiedNotes { .. } => "base-notice-filter-copied-notes",
            BasesNotice::CopyStopped { error, .. } => {
                if *error == BaseError::Cancelled {
                    "base-notice-copy-cancelled"
                } else {
                    "base-notice-copy-failed"
                }
            }
            BasesNotice::ReadOnly => "base-notice-read-only",
            BasesNotice::NothingSelected => "base-notice-nothing-selected",
            BasesNotice::Stale => "base-notice-stale",
            BasesNotice::ListReset => "base-notice-list-reset",
            BasesNotice::UnreadableEntries { .. } => "base-notice-unreadable-entries",
            BasesNotice::BrowserLimit { .. } => "base-notice-browser-limit",
        }
    }
}
