//! The book window and the opening book (spec
//! `docs/specs/m5-6-janela-do-livro.md`, D1650-D1699): two Polyglot books
//! side by side, weight tuning, adding and removing moves and lines,
//! importing a branch, creating books from the games of the filter and
//! merging them, plus the opening book that the annotation and the report
//! use.
//!
//! - `types`: [`SharedBook`], [`BookSide`], [`BookIssue`], the options and the
//!   [`BookNotice`];
//! - `message`: [`BookMessage`], with the answers of the effects;
//! - `ui`: [`BookUi`], the state of the window;
//! - `view`: [`BookView`], a snapshot ready to draw;
//! - `jobs`: the blocking work that [`perform`](crate::perform) runs;
//! - `prefs`: [`BookPrefs`] (`[book]` of the preferences).
//!
//! The flow itself (`update` of the messages, the link with the annotation
//! and the report) is in `state/book_flow.rs`.

mod jobs;
mod message;
mod prefs;
mod types;
mod ui;
mod view;

pub use jobs::BookTarget;
pub(crate) use jobs::perform_book;
#[cfg(test)]
pub(crate) use jobs::save_book;
pub use message::{BookMessage, BranchExported, CreatedBook, MergedBook, WeightScale};
pub use prefs::BookPrefs;
pub use types::{
    BookCancel, BookCreateOptions, BookExportOptions, BookIssue, BookMergeMode, BookNotice,
    BookSide, BranchStart, EXPORT_LINE_LIMIT, EXPORT_PLY_LIMIT, MAX_PLY_LIMIT, MIN_GAMES_LIMIT,
    SharedBook,
};
pub use ui::BookUi;
pub(crate) use ui::{BookSlot, Job};
pub(crate) use view::build as build_view;
pub use view::{
    BookLeadView, BookRowView, BookSideView, BookView, MAX_LEADING_ROWS, OpeningBookView,
};
