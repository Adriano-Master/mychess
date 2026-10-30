//! Bookmarks: games saved with their database and position, in nested
//! folders (spec M3-6; Scid: `tcl/file/bookmark.tcl`).
//!
//! - `tree`: the model ([`Bookmarks`]) and its edits;
//! - `storage`: `bookmarks.toml`, read and written by [`perform`](crate::perform);
//! - `find`: the best-match search that locates a game again;
//! - `message`: [`BookmarksMessage`], the editor and the notices.

mod find;
#[cfg(test)]
mod flow_tests;
mod message;
mod storage;
mod tree;

use std::path::Path;

pub(crate) use find::best_match;
pub use message::{BookmarkEditor, BookmarkNotice, BookmarksMessage, EditorMessage};
pub use storage::{
    BOOKMARKS_VERSION, BookmarksIssue, LoadedBookmarks, bookmarks_path, parse_bookmarks,
    render_bookmarks,
};
pub(crate) use storage::{load_file, save_file};
pub use tree::{
    BookmarkId, BookmarkNode, BookmarkRow, Bookmarks, Folder, GameBookmark, MAX_BOOKMARK_DEPTH,
    MAX_BOOKMARKS, MAX_TITLE_CHARS, NEW_FOLDER_TITLE, NodeKind, TreeError,
};

/// The automatic title of a bookmark (the Scid's `::bookmarks::New`):
/// `file:  White - Black, result,  City (round) year`, with surnames.
pub fn default_title(bookmark: &GameBookmark) -> String {
    let base: &Path = &bookmark.base;
    let is_si4 = base
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("si4"));
    let file = if is_si4 {
        base.file_stem()
    } else {
        base.file_name()
    }
    .map(|n| n.to_string_lossy().into_owned())
    .unwrap_or_default();
    let before_comma = |text: &str| {
        let part = text.split(',').next().unwrap_or_default().trim();
        if part.is_empty() {
            "?".to_owned()
        } else {
            part.to_owned()
        }
    };
    let mut title = format!(
        "{file}:  {} - {}, {},  {}",
        before_comma(&bookmark.white),
        before_comma(&bookmark.black),
        bookmark.result,
        before_comma(&bookmark.site),
    );
    if !bookmark.round.is_empty() && bookmark.round != "?" {
        title = format!("{title} ({})", bookmark.round);
    }
    if bookmark.year > 0 {
        title = format!("{title} {}", bookmark.year);
    }
    title.chars().take(MAX_TITLE_CHARS).collect()
}
