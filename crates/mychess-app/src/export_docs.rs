//! More formats for "Export games..." and the image of the board (spec
//! M7-4, slice b; decisions D3700-D3799 and `M7-4/D3749` onwards).
//!
//! The dialog of the M3-9 gains a format (HTML, the HTML viewer, LaTeX, EPD,
//! the game list, CSV); the job is still a job of the game list
//! (`ListJobKind::ExportGames`) and answers with the same message, so it
//! shares the progress, "Stop", the quit and the closing of a slot. The flow
//! (messages, file dialogs, confirmations, the answers) is in
//! `state/export_docs_flow.rs`; the blocking work is `run` and `image`.

mod image;
mod prefs;
mod run;

use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::{BoardMark, Position, Square};
use mychess_db::CsvColumn;
use mychess_pgn::write::WriteOptions;

use crate::board_image::{BoardPalette, ImageError, PieceSvgs};
use crate::error::{FileError, RequestId};
use crate::games_export::ExportList;

pub(crate) use image::{image_run, preview_png};
pub use prefs::{
    CsvPrefs, CsvSeparator, DEFAULT_LIST_FORMAT, EpdPrefs, ExportFormat, ExportPrefs, HtmlPrefs,
    ImageFormat, ImagePrefs, LatexPrefs, MAX_DIAGRAM_PX, MAX_LIST_FORMAT_CHARS, MAX_PER_FILE,
    MAX_TEMPLATE_BYTES, ViewerPrefs,
};
pub(crate) use run::{docs_run, fix_destination_ext};

use crate::bases::SharedSource;

/// What the host gives the export, because the application layer knows no
/// asset file: the piece drawings, the colors of the board and the font of
/// the coordinates (spec M7-4, D3723, ADR 0016).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportAssets {
    /// The drawings of the pieces of the current set.
    pub pieces: Arc<PieceSvgs>,
    /// The colors of the board of the current theme.
    pub palette: BoardPalette,
    /// A TrueType file for the coordinates of the PNG (Noto Sans).
    pub font: Arc<Vec<u8>>,
    /// `<name> by <author>, <license>` of the piece set (D3727).
    pub pieces_credit: Option<String>,
}

/// The text of the header row of the CSV, by column (the application layer
/// has no translations; the host passes them, see [`csv_column_key`]).
pub type CsvHeaders = Vec<(CsvColumn, String)>;

/// The Fluent key of the name of a CSV column.
pub fn csv_column_key(column: CsvColumn) -> &'static str {
    match column {
        CsvColumn::Number => "export-csv-col-number",
        CsvColumn::FilterNumber => "export-csv-col-filter-number",
        CsvColumn::White => "export-csv-col-white",
        CsvColumn::Black => "export-csv-col-black",
        CsvColumn::WhiteElo => "export-csv-col-white-elo",
        CsvColumn::BlackElo => "export-csv-col-black-elo",
        CsvColumn::Result => "export-csv-col-result",
        CsvColumn::Date => "export-csv-col-date",
        CsvColumn::Year => "export-csv-col-year",
        CsvColumn::Event => "export-csv-col-event",
        CsvColumn::Site => "export-csv-col-site",
        CsvColumn::Round => "export-csv-col-round",
        CsvColumn::Eco => "export-csv-col-eco",
        CsvColumn::Moves => "export-csv-col-moves",
        _ => "export-csv-col-flags",
    }
}

/// Every CSV column, in the order of the keys above (for the host to
/// translate).
pub const CSV_COLUMNS: [CsvColumn; 15] = [
    CsvColumn::Number,
    CsvColumn::FilterNumber,
    CsvColumn::White,
    CsvColumn::Black,
    CsvColumn::WhiteElo,
    CsvColumn::BlackElo,
    CsvColumn::Result,
    CsvColumn::Date,
    CsvColumn::Year,
    CsvColumn::Event,
    CsvColumn::Site,
    CsvColumn::Round,
    CsvColumn::Eco,
    CsvColumn::Moves,
    CsvColumn::Flags,
];

/// A document template to edit (D3715).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TemplateKind {
    /// The text before the games of an HTML file.
    HtmlStart,
    /// The text after the games of an HTML file.
    HtmlEnd,
    /// The preamble of a LaTeX file.
    LatexStart,
    /// The end of a LaTeX file.
    LatexEnd,
}

/// One change of an option of the export or of the image.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ExportEdit {
    /// The format of the dialog.
    Format(ExportFormat),
    /// HTML: comments.
    HtmlComments(bool),
    /// HTML: variations.
    HtmlVariations(bool),
    /// HTML: NAG symbols.
    HtmlSymbols(bool),
    /// HTML: width of the diagrams, `0` for none.
    HtmlDiagramPx(u16),
    /// Viewer: games per volume.
    ViewerPerFile(u32),
    /// Viewer: also write the PGN.
    ViewerIncludePgn(bool),
    /// Viewer: open the browser at the end.
    ViewerOpenAfter(bool),
    /// LaTeX: comments.
    LatexComments(bool),
    /// LaTeX: variations.
    LatexVariations(bool),
    /// LaTeX: board seen from Black.
    LatexFlip(bool),
    /// LaTeX: null moves as comments.
    LatexNullMoves(bool),
    /// EPD: each position once.
    EpdDedupe(bool),
    /// EPD: the final position of each game.
    EpdFinalPosition(bool),
    /// EPD: `hmvc` and `fmvn`.
    EpdHmvcFmvn(bool),
    /// EPD: an `id` operation.
    EpdId(bool),
    /// EPD: a `c0` operation with the game number.
    EpdGameNumber(bool),
    /// The format string of the game list.
    ListFormat(String),
    /// Restores the default format string of the game list.
    ResetListFormat,
    /// CSV: header row.
    CsvHeader(bool),
    /// CSV: separator.
    CsvSeparator(CsvSeparator),
    /// CSV: byte order mark.
    CsvBom(bool),
    /// A document template.
    Template(TemplateKind, String),
    /// Restores the default of a template.
    ResetTemplate(TemplateKind),
    /// Image: PNG or SVG.
    ImageFormat(ImageFormat),
    /// Image: side of the board in pixels.
    ImageSize(u32),
    /// Image: coordinates.
    ImageCoordinates(bool),
    /// Image: highlight the last move.
    ImageLastMove(bool),
    /// Image: arrows and marks.
    ImageMarks(bool),
    /// Image: transparent background.
    ImageTransparent(bool),
    /// Image: margin in pixels.
    ImageMargin(u32),
    /// Image: board seen from Black (this window only).
    ImageFlip(bool),
}

/// Events of the extra formats and of the image.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ExportDocsMessage {
    /// Chooses the format of the dialog (remembered).
    SetFormat(ExportFormat),
    /// Changes an option.
    Edit(ExportEdit),
    /// The answer to "replace / append to this file?".
    Confirm(bool),
    /// "Export position as image...": opens the dialog of the image.
    OpenImage,
    /// "Save...": asks for the file and writes the image.
    ImageSave,
    /// Closes the dialog of the image.
    ImageClose,
    /// The answer of [`Effect::RenderImage`](crate::Effect::RenderImage).
    ImageRendered {
        /// The request.
        request: RequestId,
        /// What was done.
        result: Result<ImageOutcome, ImageFailure>,
    },
    /// The answer of [`Effect::OpenExported`](crate::Effect::OpenExported).
    FileOpened {
        /// Whether the system took the file.
        ok: bool,
    },
}

/// What a job of the image did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageOutcome {
    /// The preview: PNG bytes.
    Preview(Arc<Vec<u8>>),
    /// The image was written to the file.
    Saved(PathBuf),
    /// The file exists and the dialog did not confirm replacing it; nothing
    /// was written.
    NeedsConfirm(PathBuf),
}

/// Why a job of the image failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageFailure {
    /// The image could not be made.
    Image(ImageError),
    /// The file could not be written.
    File(FileError),
    /// The file is one of the open databases.
    Protected,
    /// The task died.
    Internal,
}

impl ImageFailure {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            ImageFailure::Image(error) => error.message_key(),
            ImageFailure::File(error) => error.message_key(),
            ImageFailure::Protected => "export-err-image-protected",
            ImageFailure::Internal => "export-err-internal",
        }
    }
}

/// What a job of the image does.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageJob {
    /// What to draw (the SVG part).
    pub position: Position,
    /// Seen from Black.
    pub flip: bool,
    /// Last move to highlight.
    pub last_move: Option<(Square, Square)>,
    /// Marks and arrows (already filtered by the option).
    pub marks: Vec<BoardMark>,
    /// The options, frozen when the job was made.
    pub options: ImagePrefs,
    /// Pieces, colors and font.
    pub assets: Arc<ExportAssets>,
    /// `None` for a preview; the file for a save.
    pub dest: Option<PathBuf>,
    /// The dialog confirmed replacing `dest`.
    pub confirmed: bool,
    /// The files of the open databases.
    pub protected: Vec<PathBuf>,
}

/// Why an export did nothing, or could not go on, besides the errors of the
/// files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportProblem {
    /// Append mode: the file has no closing line (`</body>`,
    /// `\end{document}`).
    NoAppendMarker,
    /// Append mode: the file is not valid UTF-8.
    NotUtf8,
    /// The game list format has no known field.
    EmptyFormat,
    /// An option is out of its range.
    InvalidOptions,
    /// The host gave no piece drawings (the viewer needs them).
    AssetsMissing,
}

impl ExportProblem {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            ExportProblem::NoAppendMarker => "export-err-no-marker",
            ExportProblem::NotUtf8 => "export-err-not-utf8",
            ExportProblem::EmptyFormat => "export-err-empty-format",
            ExportProblem::InvalidOptions => "export-err-options",
            ExportProblem::AssetsMissing => "export-err-assets",
        }
    }
}

/// What the file already there would suffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// It would be replaced.
    Replace,
    /// The games would be added to it (HTML and LaTeX: the closing lines
    /// are replaced by the new games and the end text).
    Append,
}

/// The files that need the user's word before the job writes (D3750).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmNeeded {
    /// What would happen.
    pub kind: ConfirmKind,
    /// The existing files involved (the destination, and the `.pgn` next to
    /// a viewer).
    pub files: Vec<PathBuf>,
}

/// What a job of [`Effect::ExportDocs`](crate::Effect::ExportDocs) does.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDocsJob {
    /// The database that holds the games (not used by `Current`).
    pub source: SharedSource,
    /// The games.
    pub list: ExportList,
    /// Keep only the games with the `T` flag.
    pub only_tactics: bool,
    /// The format (never PGN).
    pub format: ExportFormat,
    /// The file, with name and extension already fixed.
    pub dest: PathBuf,
    /// The file dialog confirmed replacing exactly `dest`.
    pub dialog_confirmed: bool,
    /// The user answered "yes" to the question of [`ConfirmNeeded`].
    pub confirmed: bool,
    /// Add to the end of the file.
    pub append: bool,
    /// The options, frozen when the job was made.
    pub options: Box<ExportPrefs>,
    /// How the games go to the PGN of the viewer.
    pub write: WriteOptions,
    /// The columns of the CSV and their header text.
    pub csv_columns: Vec<(CsvColumn, String)>,
    /// Pieces, colors and font; the diagrams and the viewer need them.
    pub assets: Option<Arc<ExportAssets>>,
    /// The title of the viewer pages.
    pub title: String,
    /// The files of the databases that are open.
    pub protected: Vec<PathBuf>,
}

/// The question about a file that exists (D3750).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfirmPrompt {
    pub(crate) needed: ConfirmNeeded,
    pub(crate) job: Box<ExportDocsJob>,
    pub(crate) slot: crate::bases::SlotId,
}

/// What the image window is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageStep {
    /// The options and the preview.
    Options,
    /// The file dialog is open.
    Picking(RequestId),
    /// The image is being written.
    Saving(RequestId),
    /// The file exists: waiting for the user's word.
    Confirm(PathBuf),
}

/// The window of the board image (D3724).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImageWindow {
    pub(crate) position: Position,
    pub(crate) last_move: Option<(Square, Square)>,
    pub(crate) marks: Vec<BoardMark>,
    pub(crate) flip: bool,
    /// The name of the database, to propose a file name.
    pub(crate) base_name: String,
    pub(crate) step: ImageStep,
    /// The preview asked for last; an older answer is dropped (D3728).
    pub(crate) preview_request: Option<RequestId>,
    pub(crate) preview: Option<Arc<Vec<u8>>>,
    pub(crate) preview_error: Option<ImageFailure>,
}

/// The state of the extra formats and of the image in
/// [`AppState`](crate::AppState).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportDocsUi {
    pub(crate) assets: Option<Arc<ExportAssets>>,
    pub(crate) csv_headers: CsvHeaders,
    /// The job in flight, kept to ask again after a confirmation.
    pub(crate) last_job: Option<Box<ExportDocsJob>>,
    pub(crate) confirm: Option<ConfirmPrompt>,
    pub(crate) image: Option<ImageWindow>,
    /// Changes made while the preferences file was still being read; they
    /// are applied over the loaded values (D3751), one field at a time so
    /// the other values of the file stay.
    pub(crate) early_edits: Vec<ExportEdit>,
}

impl ExportDocsUi {
    /// Whether a question or the image window is open.
    pub fn is_open(&self) -> bool {
        self.confirm.is_some() || self.image.is_some()
    }
}

/// The question the dialog of the export asks.
#[derive(Debug, Clone, Copy)]
pub struct ExportConfirmView<'a> {
    /// What would happen to the files.
    pub kind: ConfirmKind,
    /// The format being written.
    pub format: ExportFormat,
    /// The existing files involved.
    pub files: &'a [PathBuf],
}

/// What the dialog of the export shows besides the options of the M3-9.
#[derive(Debug, Clone, Copy)]
pub struct ExportDocsView<'a> {
    /// The format and the options of each format.
    pub prefs: &'a ExportPrefs,
    /// Whether the host gave the piece drawings (the viewer, the diagrams
    /// and the image need them).
    pub has_assets: bool,
    /// The question to answer, if one is open.
    pub confirm: Option<ExportConfirmView<'a>>,
}

/// The step of the image window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageStepView<'a> {
    /// The options.
    Options,
    /// Waiting for the file dialog.
    Picking,
    /// Writing the file.
    Saving,
    /// Replace this file?
    Confirm(&'a std::path::Path),
}

/// What the image window shows.
#[derive(Debug, Clone, Copy)]
pub struct ExportImageView<'a> {
    /// The options.
    pub prefs: &'a ImagePrefs,
    /// Seen from Black's side.
    pub flip: bool,
    /// The step.
    pub step: ImageStepView<'a>,
    /// The PNG of the preview, once rendered.
    pub preview: Option<&'a Arc<Vec<u8>>>,
    /// Why the preview failed.
    pub preview_error: Option<&'a ImageFailure>,
    /// The position of the image (the one shown when the window opened); the
    /// front-end draws its own SVG preview from it (spec M7-4c).
    pub position: &'a Position,
    /// The last move to highlight, if any.
    pub last_move: Option<(Square, Square)>,
    /// The arrows and marks of the comment.
    pub marks: &'a [BoardMark],
}

#[cfg(test)]
mod tests;
