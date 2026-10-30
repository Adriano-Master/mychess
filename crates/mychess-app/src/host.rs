//! Requests to the front-end that need the operating system: file dialogs
//! and the clipboard (spec M2-2, D100).

use std::path::PathBuf;

use crate::error::RequestId;

/// What the file dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PickKind {
    /// Choose a PGN file to open (filter `*.pgn`, `*.pgn.gz`, all files).
    OpenPgn,
    /// Choose where to save the game (the dialog confirms overwriting).
    SavePgn,
    /// Choose a PGN file to append the game to (it may not exist yet).
    AppendPgn,
    /// Choose an `.eco` opening file.
    OpenEco,
    /// Choose a database or PGN file to open as a locked opening tree.
    OpenTreeBase,
    /// Choose a tree mask file (`*.stm`) to open (spec M5-4b).
    OpenMask,
    /// Choose where to save the tree mask (`*.stm`; the dialog confirms
    /// overwriting).
    SaveMask,
    /// Choose one or more PGN files to import (spec M3-8; the answer is
    /// [`Message::FilesPicked`](crate::Message::FilesPicked)).
    ImportPgn,
    /// Choose the name of a new database (`*.si4`; spec M3-8).
    NewBase,
    /// Choose a folder of Syzygy tablebase files (spec M8-8, D2573).
    TablebaseDir,
    /// Choose a spelling file (`*.ssp`, `*.ssp.gz`; spec M6-3).
    OpenSpelling,
    /// Choose an EPD file to open (`*.epd`, `*.epd.gz`; spec M7-5).
    OpenEpd,
    /// Choose the name of a new EPD file (`*.epd`; the dialog confirms
    /// overwriting; spec M7-5).
    NewEpd,
    /// Choose where to save the EPD file under another name (`*.epd`,
    /// `*.epd.gz`; the dialog confirms overwriting; spec M7-5).
    SaveEpd,
    /// Choose where to save an exported document of `format`, or the file to
    /// add the games to when `append` (spec M7-4; the dialog confirms
    /// replacing only for a save).
    ExportDoc {
        /// The format, which says the extension and the filter.
        format: crate::export_docs::ExportFormat,
        /// The games go to the end of a file that may not exist yet.
        append: bool,
    },
    /// Choose where to save the image of the board (spec M7-4; the dialog
    /// confirms replacing).
    ExportImage {
        /// PNG or SVG.
        format: crate::export_docs::ImageFormat,
    },
}

/// A file dialog request. The answer is `Message::FilePicked(request,
/// path)`; `None` means the user cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickSpec {
    /// Correlates the answer.
    pub request: RequestId,
    /// What the dialog is for.
    pub kind: PickKind,
    /// The directory to start in (the last one used).
    pub initial_dir: Option<PathBuf>,
    /// The file name to propose when saving.
    pub file_name: Option<String>,
}
