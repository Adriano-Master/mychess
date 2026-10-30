//! Messages, the editor and the notices of the bookmarks (spec M3-6,
//! D678, D680).

use super::storage::LoadedBookmarks;
use super::tree::{BookmarkId, Bookmarks};
use crate::bases::BaseError;
use crate::error::{FileError, RequestId};

/// An event of the bookmarks.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BookmarksMessage {
    /// Bookmark the game being edited.
    AddCurrent,
    /// Open the bookmarked game.
    Go(BookmarkId),
    /// Open the editor.
    OpenEditor,
    /// An event of the editor.
    Edit(EditorMessage),
    /// The file was read.
    Loaded(LoadedBookmarks),
    /// The file was written.
    Saved(Result<(), FileError>),
    /// The best game for a bookmark was found (`None`: the database is
    /// empty).
    Found {
        /// The request.
        request: RequestId,
        /// The game number, from 1.
        result: Result<Option<u32>, BaseError>,
    },
}

/// An event of the editor window.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditorMessage {
    /// Select an entry (or none).
    Select(Option<BookmarkId>),
    /// The text of the title field changed.
    Rename(String),
    /// Swap with the previous sibling.
    MoveUp,
    /// Swap with the next sibling.
    MoveDown,
    /// Move into the folder before it.
    Indent,
    /// Move out of its folder.
    Outdent,
    /// Add a folder after the selection.
    NewFolder,
    /// Add the game being edited after the selection.
    AddCurrent,
    /// Delete the selection (a folder with what is inside).
    Delete,
    /// Keep the edits, close, and open the selected game.
    Go,
    /// Keep the edits and close.
    Ok,
    /// Discard the edits and close.
    Cancel,
}

/// The editor: a working copy of the tree (OK keeps it, Cancel drops it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkEditor {
    /// The tree being edited.
    pub working: Bookmarks,
    /// The selected entry.
    pub selected: Option<BookmarkId>,
    /// The text of the title field (the title of the selection).
    pub title: String,
}

/// A remark about the bookmarks for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookmarkNotice {
    /// A bookmark was added.
    Added,
    /// The game being edited cannot be bookmarked (not a game of an open
    /// database).
    CannotAdd,
    /// The path of the database is not valid UTF-8.
    PathNotUtf8,
    /// The tree is full or folders are nested too deeply.
    TreeFull,
    /// The database of the bookmark could not be opened.
    BaseUnavailable {
        /// The title of the bookmark.
        title: String,
    },
    /// The database has no game to go to.
    GameNotFound {
        /// The title of the bookmark.
        title: String,
    },
    /// Entries of the file were left out.
    Dropped {
        /// How many.
        count: usize,
    },
    /// The file could not be read; it was set aside.
    FileUnreadable,
    /// The file was written by a newer version.
    NewerVersion,
    /// The file could not be written.
    SaveFailed,
}

impl BookmarkNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            BookmarkNotice::Added => "bookmark-notice-added",
            BookmarkNotice::CannotAdd => "bookmark-notice-cannot-add",
            BookmarkNotice::PathNotUtf8 => "bookmark-notice-path-not-utf8",
            BookmarkNotice::TreeFull => "bookmark-notice-tree-full",
            BookmarkNotice::BaseUnavailable { .. } => "bookmark-notice-base-unavailable",
            BookmarkNotice::GameNotFound { .. } => "bookmark-notice-game-not-found",
            BookmarkNotice::Dropped { .. } => "bookmark-notice-dropped",
            BookmarkNotice::FileUnreadable => "bookmark-notice-file-unreadable",
            BookmarkNotice::NewerVersion => "bookmark-notice-newer-version",
            BookmarkNotice::SaveFailed => "bookmark-notice-save-failed",
        }
    }
}
