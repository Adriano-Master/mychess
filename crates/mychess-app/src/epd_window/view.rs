//! What the front-end draws of the EPD window (spec M7-5, D3801, D3814).
//!
//! [`AppState::epd_view`](crate::AppState::epd_view) is cheap: it does not
//! walk the records. The rows of the list are asked for a page at a time
//! with [`AppState::epd_rows`](crate::AppState::epd_rows).

use std::ops::Range;
use std::path::PathBuf;

use mychess_core::Position;
use mychess_pgn::epd::{EpdBook, EpdIndex};

use super::file::{EpdFile, EpdUi, RunStage};
use super::message::{EpdFileId, EpdSort};
use super::prefs::AnnotateMode;
use super::run::{RunOutcome, RunProgress};

/// The analysis in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct EpdRunView {
    /// The mode.
    pub mode: AnnotateMode,
    /// The name of the engine.
    pub engine: String,
    /// Seconds per position.
    pub seconds: u32,
    /// The engine is still starting.
    pub starting: bool,
    /// The user asked to stop and the run waits for the engine.
    pub stopping: bool,
    /// The progress.
    pub progress: RunProgress,
}

/// A tab of the window: one open file.
#[derive(Debug, Clone, PartialEq)]
pub struct EpdTabView {
    /// The file.
    pub id: EpdFileId,
    /// Where it is.
    pub path: PathBuf,
    /// The name to show on the tab.
    pub name: String,
    /// The file cannot be edited or saved over (Save as still works).
    pub read_only: bool,
    /// It was altered since it was read or saved.
    pub dirty: bool,
    /// A job holds the file: edits wait for it.
    pub busy: bool,
    /// Records in the file.
    pub records: usize,
    /// Records in the list (after the filter).
    pub rows: usize,
    /// The order and the filter of the list.
    pub sort: EpdSort,
    /// The filter text.
    pub filter: String,
    /// The position of the selected record in the list.
    pub selected_row: Option<usize>,
    /// The selected record.
    pub selected: Option<EpdIndex>,
    /// The text of the editor.
    pub editor_text: String,
    /// The text of the editor was changed and not yet written to the record.
    pub editor_draft: bool,
    /// Whether the editor accepts text.
    pub editor_enabled: bool,
    /// The four-field FEN of the selected record.
    pub fen: Option<String>,
    /// The moves from the position of the board that lead to positions of
    /// the file, in SAN, in ASCII order (`[k: ...]` of the status bar).
    pub book_moves: Vec<String>,
    /// The text of the last search.
    pub find: String,
    /// Where the last search matched, as bytes of `FEN` + `\n` + the
    /// operations text (the span of `EpdRecord::search_text`).
    pub find_hit: Option<Range<usize>>,
    /// Lines of the file that were ignored or repaired when it was read
    /// (kept ones plus the dropped).
    pub skipped_lines: u64,
    /// Opcodes of the last commit that are not standard.
    pub nonstandard: Vec<String>,
    /// The analysis, if it runs on this file.
    pub run: Option<EpdRunView>,
    /// How the last analysis on this file ended, with its numbers.
    pub last_run: Option<(RunOutcome, RunProgress)>,
    /// "Undo analysis" is available.
    pub can_undo_run: bool,
    /// The record whose removal waits for the answer.
    pub pending_remove: Option<EpdIndex>,
}

/// The EPD window.
#[derive(Debug, Clone, PartialEq)]
pub struct EpdView {
    /// The tabs, in the order the files were opened.
    pub tabs: Vec<EpdTabView>,
    /// The active tab.
    pub active: Option<EpdFileId>,
    /// The most files that can be open at once.
    pub max_files: usize,
    /// A file is being read or created.
    pub opening: bool,
    /// The file whose "save before closing?" question is open.
    pub close_prompt: Option<EpdFileId>,
    /// The recent files, newest first.
    pub recent: Vec<PathBuf>,
}

/// A row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpdRowView {
    /// The number shown, from 1, in the order of the list.
    pub number: usize,
    /// The record.
    pub index: EpdIndex,
    /// The four-field FEN.
    pub fen: String,
    /// The `id` of the record, if it has one.
    pub id: Option<String>,
}

/// The tab of a file. `detailed` (the tab that is shown) also computes the
/// row of the selection (O(n) when the list is not in file order) and the
/// moves of the book; the other tabs show only their name and flags.
pub(crate) fn tab_view(file: &EpdFile, ui: &EpdUi, board: &Position, detailed: bool) -> EpdTabView {
    let run = ui.run.as_ref().filter(|r| r.file == file.id).map(|r| {
        let (progress, stopping, starting) = match &r.stage {
            RunStage::Starting => (RunProgress::default(), false, true),
            RunStage::Running(run) => (run.progress(), run.is_cancelling(), false),
        };
        EpdRunView {
            mode: r.config.mode,
            engine: r.engine_name.clone(),
            seconds: r.config.seconds,
            starting,
            stopping,
            progress,
        }
    });
    let editor_enabled = !file.read_only && !file.locked() && run.is_none();
    let nonstandard = file
        .warnings
        .iter()
        .map(|w| match w {
            mychess_pgn::epd::TextWarning::NonStandardOpcode { opcode, .. } => opcode.clone(),
            _ => String::new(),
        })
        .collect();
    let name = file
        .path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    EpdTabView {
        id: file.id,
        path: file.path.clone(),
        name,
        read_only: file.read_only,
        dirty: file.dirty,
        busy: file.locked(),
        records: file.book.len(),
        rows: file.rows.len(),
        sort: file.sort,
        filter: file.filter.clone(),
        selected_row: file
            .selected
            .filter(|_| detailed)
            .and_then(|i| file.row_of(i)),
        selected: file.selected,
        editor_text: file.editor_text(),
        editor_draft: file.draft.is_some(),
        editor_enabled,
        fen: file.selected_position().map(four_field_fen),
        book_moves: if detailed {
            book_moves(&file.book, board)
        } else {
            Vec::new()
        },
        find: file.find_text.clone(),
        find_hit: file.find_hit.clone(),
        skipped_lines: u64::try_from(file.report.diagnostics.len())
            .unwrap_or(u64::MAX)
            .saturating_add(file.report.dropped),
        nonstandard,
        run,
        last_run: file.last_run,
        can_undo_run: file.undo.is_some(),
        pending_remove: file.pending_remove,
    }
}

/// The FEN without the clocks.
pub(crate) fn four_field_fen(position: &Position) -> String {
    position
        .fen()
        .split_ascii_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The legal moves of `board` that lead to a position of `book`, in SAN,
/// in ASCII order.
pub(crate) fn book_moves(book: &EpdBook, board: &Position) -> Vec<String> {
    let mut moves: Vec<String> = book
        .moves_in_book(board)
        .into_iter()
        .filter_map(|m| board.san(m).ok())
        .collect();
    moves.sort();
    moves
}

pub(crate) fn row_views(file: &EpdFile, range: Range<usize>) -> Vec<EpdRowView> {
    let end = range.end.min(file.rows.len());
    let start = range.start.min(end);
    file.rows[start..end]
        .iter()
        .enumerate()
        .filter_map(|(offset, index)| {
            let record = file.book.get_by_index(*index)?;
            Some(EpdRowView {
                number: start + offset + 1,
                index: *index,
                fen: four_field_fen(record.position()),
                id: record.id().map(str::to_owned),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fen_has_four_fields() {
        assert_eq!(
            four_field_fen(&Position::new()),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -"
        );
    }

    #[test]
    fn book_moves_are_sorted_san() {
        let mut book = EpdBook::new();
        let start = Position::new();
        for san in ["e4", "d4", "Nf3"] {
            let m = start
                .parse_san(san, mychess_core::SanMode::Tolerant)
                .expect("san")
                .mv;
            book.insert(start.after(m).expect("after"), Vec::new());
        }
        assert_eq!(book_moves(&book, &start), ["Nf3", "d4", "e4"]);
        assert!(book_moves(&EpdBook::new(), &start).is_empty());
    }
}
