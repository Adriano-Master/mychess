//! What the front-end draws of the masks: snapshots of plain data (spec
//! M5-4, D1827 to D1829, D1864).
//!
//! A comment is shown as its first line (at most
//! [`MAX_PREVIEW_BYTES`](mychess_db::tree::mask::MAX_PREVIEW_BYTES) bytes)
//! plus a flag telling that there is more; the whole text comes from
//! [`AppState::mask_comment`](crate::AppState::mask_comment) when a tool tip
//! or an editor needs it, so a snapshot taken at every frame never copies
//! 64 KiB per row.

use std::path::PathBuf;
use std::sync::Arc;

use mychess_db::tree::mask::{Marker, MaskColor, MaskMove, MaskNag, StmReport};

/// The first line of `text`, cut at `max` bytes on a character boundary.
pub(crate) fn preview(text: &str, max: usize) -> (String, bool) {
    let line = text.lines().next().unwrap_or("");
    let more = text
        .get(line.len()..)
        .is_some_and(|rest| !rest.trim().is_empty());
    if line.len() <= max {
        return (line.to_owned(), more);
    }
    let mut end = max;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    (line.get(..end).unwrap_or_default().to_owned(), true)
}

/// What the mask says about a move: the cell of a row (D1827).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskCell {
    /// The move is in the mask (its text is drawn with the highlight).
    pub in_mask: bool,
    /// The NAG.
    pub nag: Option<MaskNag>,
    /// The color.
    pub color: MaskColor,
    /// The two markers.
    pub markers: [Option<Marker>; 2],
    /// The first line of the comment.
    pub comment: String,
    /// The comment has more than the line shown (a tool tip is worth it).
    pub comment_more: bool,
}

impl MaskCell {
    /// The cell of an entry of the mask.
    pub(crate) fn of(entry: &MaskMove) -> MaskCell {
        let (comment, comment_more) =
            preview(&entry.comment, mychess_db::tree::mask::MAX_PREVIEW_BYTES);
        MaskCell {
            in_mask: true,
            nag: entry.nag.clone(),
            color: entry.color.clone(),
            markers: entry.markers.clone(),
            comment,
            comment_more,
        }
    }
}

/// A move that is in the mask and not in the tree (D1828), drawn after the
/// `TOTAL` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskOnlyRow {
    /// The index to send in
    /// [`MaskTarget::MaskOnly`](crate::MaskTarget::MaskOnly).
    pub index: usize,
    /// The move as the mask stores it.
    pub san: String,
    /// What the mask says about it.
    pub cell: MaskCell,
    /// The move is legal in the position. An illegal entry is drawn with a
    /// warning (case 12 of the spec) and a click does nothing.
    pub legal: bool,
    /// A click plays the move now (legal, and the table follows the game).
    pub clickable: bool,
}

/// The kind of job that runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaskJobKind {
    /// Reading a file.
    Open,
    /// Writing a file.
    Save,
    /// Filling from the games of the filter.
    Fill,
    /// Searching the mask.
    Search,
    /// Building the outline.
    Outline,
}

/// A job of the mask that runs, with its progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskBusy {
    /// The kind.
    pub kind: MaskJobKind,
    /// Games done (a fill; 0 for the others).
    pub done: u64,
    /// Games to do (0 while unknown).
    pub total: u64,
    /// Stopping was asked.
    pub cancelling: bool,
}

/// The mask part of the tree panel (spec M5-4, D1830).
#[derive(Debug, Clone, PartialEq)]
pub struct MaskView {
    /// The file name with its extension; `None` for a mask with no file.
    pub file_name: Option<String>,
    /// The file.
    pub path: Option<PathBuf>,
    /// The mask was altered since it was read or written: the title shows a
    /// dot.
    pub dirty: bool,
    /// Positions in the mask.
    pub positions: usize,
    /// What the reader found when the file was read; the status bar shows
    /// "N warnings" and the tool tip lists them (D1814).
    pub report: Arc<StmReport>,
    /// Draw the markers, the color and the NAG (D1831).
    pub show_markers: bool,
    /// Show the whole comment in a tool tip.
    pub tooltips: bool,
    /// "Load the last mask when the tree opens".
    pub auto_load: bool,
    /// The recent files, newest first (at most 10).
    pub recent: Vec<PathBuf>,
    /// The game is standard chess; with `false` the menu is off (D1835).
    pub enabled: bool,
    /// The first line of the comment of the position of the table (the
    /// strip above it, D1829); empty if none.
    pub position_comment: String,
    /// The comment of the position has more than that line.
    pub position_comment_more: bool,
    /// The moves only the mask has, in mask order. Empty when the tree is
    /// unavailable, being searched for the first time, failed, or during an
    /// online game or the training.
    pub only_rows: Vec<MaskOnlyRow>,
    /// The job that runs, if any.
    pub busy: Option<MaskBusy>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_is_the_first_line_and_says_when_there_is_more() {
        assert_eq!(preview("one", 200), ("one".to_owned(), false));
        assert_eq!(preview("one\ntwo", 200), ("one".to_owned(), true));
        assert_eq!(preview("one\n  \n", 200), ("one".to_owned(), false));
        assert_eq!(preview("", 200), (String::new(), false));
    }

    #[test]
    fn a_preview_is_cut_on_a_character_boundary() {
        let (cut, more) = preview("aéé", 2);
        assert_eq!(cut, "a");
        assert!(more);
    }
}
