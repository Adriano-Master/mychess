//! The events of the book window (spec M5-6).

use std::path::PathBuf;

use mychess_core::Move;

use super::types::{
    BookCreateOptions, BookExportOptions, BookIssue, BookMergeMode, BookSide, SharedBook,
};
use crate::error::RequestId;
use crate::play::PlayBook;

/// How a weight changes with [`BookMessage::ScaleWeight`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightScale {
    /// Twice the weight.
    Double,
    /// Half the weight.
    Half,
}

/// A book made from games (answer of [`Effect::BookCreate`](crate::Effect)).
#[derive(Debug, Clone, PartialEq)]
pub struct CreatedBook {
    /// The new book.
    pub book: SharedBook,
    /// Games used.
    pub used: u64,
    /// Games left out.
    pub skipped: u64,
}

/// Two books merged (answer of [`Effect::BookMerge`](crate::Effect)).
#[derive(Debug, Clone, PartialEq)]
pub struct MergedBook {
    /// The result.
    pub book: SharedBook,
    /// Entries written.
    pub written: usize,
    /// Entries of the second book that were left out.
    pub skipped: usize,
}

/// A branch written as PGN (answer of [`Effect::BookExport`](crate::Effect)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchExported {
    /// The file written.
    pub path: PathBuf,
    /// Lines (games) written.
    pub lines: usize,
    /// The limit of lines cut the branch short.
    pub truncated: bool,
}

/// An event of the book window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BookMessage {
    /// The panel was opened or closed by the front-end.
    SetOpen(bool),
    /// Reads the book at `path` into `side`.
    Load {
        /// The side.
        side: BookSide,
        /// The file.
        path: PathBuf,
    },
    /// An empty book in `side`.
    New {
        /// The side.
        side: BookSide,
    },
    /// Closes the book of `side`; with unsaved changes the window asks
    /// first (`BookView::confirm_close`).
    Close {
        /// The side.
        side: BookSide,
    },
    /// The answer to "close the book with unsaved changes?".
    ConfirmClose(bool),
    /// Sets the weight of `mv` in the position of the cursor from the text
    /// of a field (`1..=65520`).
    SetWeight {
        /// The side.
        side: BookSide,
        /// The move.
        mv: Move,
        /// What was typed.
        text: String,
    },
    /// Doubles or halves the weight of `mv`.
    ScaleWeight {
        /// The side.
        side: BookSide,
        /// The move.
        mv: Move,
        /// Which.
        scale: WeightScale,
    },
    /// Removes `mv` from the position of the cursor.
    RemoveMove {
        /// The side.
        side: BookSide,
        /// The move.
        mv: Move,
    },
    /// Adds `mv` (weight 1) to the position of the cursor.
    AddMove {
        /// The side.
        side: BookSide,
        /// The move.
        mv: Move,
    },
    /// Adds the move that the game plays after the cursor.
    AddGameMove {
        /// The side.
        side: BookSide,
    },
    /// Adds the main continuation of the game from the cursor.
    AddLine {
        /// The side.
        side: BookSide,
    },
    /// Removes the main continuation of the game from the cursor.
    RemoveLine {
        /// The side.
        side: BookSide,
    },
    /// Adds every line of the subtree of the cursor, variations included.
    ImportBranch {
        /// The side.
        side: BookSide,
    },
    /// Copies the moves of the position of the cursor from the other book.
    CopyPosition {
        /// The book that receives them.
        to: BookSide,
    },
    /// Saves the book of `side` in its file.
    Save {
        /// The side.
        side: BookSide,
    },
    /// Saves the book of `side` in `path` and binds it there.
    SaveAs {
        /// The side.
        side: BookSide,
        /// The file.
        path: PathBuf,
    },
    /// Makes a book from the games of the filter of the current database.
    CreateFromFilter {
        /// The side that receives it.
        side: BookSide,
    },
    /// Stops the making of a book from the filter; the partial result is
    /// dropped and no file is left (D1791).
    CancelCreate,
    /// Writes the branch of the book of `side` that starts at the cursor as
    /// PGN, one game per line, to `path` (D2160).
    ExportBranch {
        /// The side.
        side: BookSide,
        /// The file chosen in the dialog.
        path: PathBuf,
    },
    /// Chooses the options of the export.
    SetExportOptions(BookExportOptions),
    /// Stops the export; the file is left as it was (D2165).
    CancelExport,
    /// Merges the first book with the second into the first.
    Merge,
    /// Chooses the merge mode.
    SetMergeMode(BookMergeMode),
    /// Changes the options of "create".
    SetCreateOptions(BookCreateOptions),
    /// Uses the file of the book of `side` as the opening book.
    UseAsOpening(BookSide),
    /// Uses no opening book.
    ClearOpening,
    /// Hides the last notice.
    DismissNotice,

    // ---- answers to effects ----
    /// A book of a side was read.
    SlotLoaded {
        /// The request.
        request: RequestId,
        /// The side.
        side: BookSide,
        /// The book, or why not.
        result: Result<SharedBook, BookIssue>,
    },
    /// The opening book was read.
    OpeningLoaded {
        /// The request.
        request: RequestId,
        /// The book, or why not.
        result: Result<PlayBook, BookIssue>,
    },
    /// A book was written.
    Saved {
        /// The request.
        request: RequestId,
        /// The saved book (bound to its file, clean), or why not.
        result: Result<SharedBook, BookIssue>,
    },
    /// A book was made from games.
    Created {
        /// The request.
        request: RequestId,
        /// The book, or why not.
        result: Result<CreatedBook, BookIssue>,
    },
    /// Two books were merged.
    Merged {
        /// The request.
        request: RequestId,
        /// The result, or why not.
        result: Result<MergedBook, BookIssue>,
    },
    /// A branch was exported.
    Exported {
        /// The request.
        request: RequestId,
        /// The outcome, or why not.
        result: Result<BranchExported, BookIssue>,
    },
}
