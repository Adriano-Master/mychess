//! A ready-to-draw snapshot of the book window (spec M5-6, D1651, D1653).

use std::path::PathBuf;

use mychess_core::{CastlingMode, Game, Move, NodeId};

use super::types::{
    BookCreateOptions, BookExportOptions, BookIssue, BookMergeMode, BookNotice, BookSide,
};
use super::ui::BookUi;

/// The most "other moves" listed under a book.
pub const MAX_LEADING_ROWS: usize = 40;

/// A move of a book in the position of the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookRowView {
    /// The move.
    pub mv: Move,
    /// Its SAN.
    pub san: String,
    /// The weight in the file.
    pub weight: u16,
    /// The share of the position, in percent (the Scid's rounding).
    pub percent: u8,
    /// The game plays this move next.
    pub in_game: bool,
}

/// A move that is not in the book but leads to a position that is (D1653).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookLeadView {
    /// The move.
    pub mv: Move,
    /// Its SAN.
    pub san: String,
}

/// One book of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSideView {
    /// Which book.
    pub side: BookSide,
    /// The name of the file; `None` for a book without a file.
    pub name: Option<String>,
    /// The file.
    pub path: Option<PathBuf>,
    /// There are changes that are not saved.
    pub dirty: bool,
    /// The book cannot be edited.
    pub read_only: bool,
    /// Records in the book.
    pub entries: usize,
    /// The moves of the position of the cursor, in the order of the file.
    pub rows: Vec<BookRowView>,
    /// The moves that lead to a position of the book.
    pub leading: Vec<BookLeadView>,
    /// The file is the opening book.
    pub is_opening: bool,
}

/// The opening book of the annotation and the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningBookView {
    /// The file chosen in the preferences.
    pub path: PathBuf,
    /// The name shown in the comments, when the book is loaded.
    pub loaded_name: Option<String>,
    /// Why it is not loaded.
    pub issue: Option<BookIssue>,
}

/// What the front-end draws.
#[derive(Debug, Clone, PartialEq)]
pub struct BookView {
    /// The two books, left first.
    pub sides: [Option<BookSideView>; 2],
    /// The position of the cursor can be in a book (not Chess960).
    pub standard: bool,
    /// The SAN of the move the game plays next, if there is one.
    pub game_move: Option<String>,
    /// The game continues after the cursor.
    pub has_continuation: bool,
    /// The merge mode.
    pub merge_mode: BookMergeMode,
    /// The options of "create".
    pub create: BookCreateOptions,
    /// Games in the filter of the current database.
    pub filter_games: u32,
    /// The opening book, if the preferences name one.
    pub opening: Option<OpeningBookView>,
    /// The last notice.
    pub notice: Option<BookNotice>,
    /// A job is running.
    pub busy: bool,
    /// A book is being made from the games of the filter (it can be
    /// stopped).
    pub creating: bool,
    /// The options of the export of a branch to PGN.
    pub export: BookExportOptions,
    /// A branch is being written; `Some((done, total))` is its progress
    /// (the total is 0 until the lines are counted).
    pub export_progress: Option<(u64, u64)>,
    /// The export was asked to stop and has not answered yet (D2170).
    pub export_stopping: bool,
    /// The side whose close is waiting for an answer.
    pub confirm_close: Option<BookSide>,
}

/// Builds the view of `ui` for the position of `cursor` in `game`.
pub(crate) fn build(
    ui: &BookUi,
    game: &Game,
    cursor: NodeId,
    opening_pref: Option<&PathBuf>,
    filter_games: u32,
) -> BookView {
    let position = game.position_at(cursor).ok();
    let standard = position.is_some_and(|p| p.castling_mode() == CastlingMode::Standard);
    let next = game
        .children(cursor)
        .ok()
        .and_then(|c| c.first().copied())
        .and_then(|n| game.mv(n).ok());
    let game_move = next.and_then(|mv| position.and_then(|p| p.san(mv).ok()));
    let sides = BookSide::ALL.map(|side| {
        let slot = ui.slot(side)?;
        let book = slot.book.get();
        let (mut rows, mut leading) = (Vec::new(), Vec::new());
        if let (true, Some(p)) = (standard, position) {
            rows = book
                .moves(p)
                .into_iter()
                .filter_map(|m| {
                    Some(BookRowView {
                        mv: m.mv,
                        san: p.san(m.mv).ok()?,
                        weight: m.weight,
                        percent: m.percent,
                        in_game: next == Some(m.mv),
                    })
                })
                .collect();
            leading = book
                .leading_to_book(p)
                .into_iter()
                .filter(|mv| !rows.iter().any(|r| r.mv == *mv))
                .filter_map(|mv| {
                    Some(BookLeadView {
                        san: p.san(mv).ok()?,
                        mv,
                    })
                })
                .take(MAX_LEADING_ROWS)
                .collect();
        }
        let path = book.path().map(std::path::Path::to_path_buf);
        Some(BookSideView {
            side,
            name: path
                .as_deref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            is_opening: path.is_some() && path.as_ref() == opening_pref,
            path,
            dirty: slot.is_dirty(),
            read_only: book.is_read_only(),
            entries: book.len(),
            rows,
            leading,
        })
    });
    let opening = opening_pref.map(|path| OpeningBookView {
        path: path.clone(),
        loaded_name: ui.opening.as_ref().map(|b| b.book_name().to_owned()),
        issue: ui.opening_issue,
    });
    BookView {
        sides,
        standard,
        game_move,
        has_continuation: next.is_some(),
        merge_mode: ui.merge_mode,
        create: ui.create,
        filter_games,
        opening,
        notice: ui.notice,
        busy: ui.is_busy(),
        creating: ui.is_creating(),
        export: ui.export,
        export_progress: ui.export_progress(),
        export_stopping: ui.is_export_stopping(),
        confirm_close: ui.confirm_close,
    }
}
