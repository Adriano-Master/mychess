//! The `[export]` table of the preferences (spec M7-4, D3729): the format
//! and the options of each format, the options of the image and the document
//! templates. A wrong value goes back to the default of its own field.

use serde::{Serialize, Serializer};

use crate::board_image::{MAX_MARGIN_PX, SIZE_RANGE};
use crate::panel_calc::named_enum;

named_enum! {
    /// The formats of "Export games..." (D3700).
    pub enum ExportFormat {
        /// PGN, the export of the M3-9.
        #[default]
        Pgn => "pgn",
        /// A plain HTML document.
        Html => "html",
        /// The HTML viewer, in volumes.
        Viewer => "viewer",
        /// A LaTeX document (`xskak`).
        Latex => "latex",
        /// The positions of the games, as EPD.
        Epd => "epd",
        /// One text line per game.
        List => "list",
        /// The game list as CSV.
        Csv => "csv",
    }
}

named_enum! {
    /// The file format of the board image (D3723).
    pub enum ImageFormat {
        /// A PNG, rasterized by `resvg`.
        #[default]
        Png => "png",
        /// A SVG document.
        Svg => "svg",
    }
}

named_enum! {
    /// The field separator of the CSV export.
    pub enum CsvSeparator {
        /// A comma.
        #[default]
        Comma => "comma",
        /// A semicolon (spreadsheets of some locales).
        Semicolon => "semicolon",
    }
}

macro_rules! serialize_by_name {
    ($($t:ty),+) => {$(
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }
    )+};
}
serialize_by_name!(ExportFormat, ImageFormat, CsvSeparator);

impl ExportFormat {
    /// The extension a file of the format gets, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Pgn => "pgn",
            ExportFormat::Html | ExportFormat::Viewer => "html",
            ExportFormat::Latex => "tex",
            ExportFormat::Epd => "epd",
            ExportFormat::List => "txt",
            ExportFormat::Csv => "csv",
        }
    }

    /// The extensions that count as the format's own (any case); the others
    /// are kept and the proper one is added.
    pub fn accepted_extensions(self) -> &'static [&'static str] {
        match self {
            ExportFormat::Pgn => &["pgn"],
            ExportFormat::Html | ExportFormat::Viewer => &["html", "htm"],
            ExportFormat::Latex => &["tex", "ltx"],
            ExportFormat::Epd => &["epd"],
            ExportFormat::List => &["txt"],
            ExportFormat::Csv => &["csv"],
        }
    }

    /// Whether the format can be appended to an existing file (D3722: not
    /// the viewer, which makes new volumes).
    pub fn can_append(self) -> bool {
        !matches!(self, ExportFormat::Viewer)
    }

    /// Whether the format needs a game of a database (the lists read the
    /// index entry of each game).
    pub fn needs_database(self) -> bool {
        matches!(self, ExportFormat::List | ExportFormat::Csv)
    }

    /// The Fluent key of the name of the format.
    pub fn message_key(self) -> &'static str {
        match self {
            ExportFormat::Pgn => "export-format-pgn",
            ExportFormat::Html => "export-format-html",
            ExportFormat::Viewer => "export-format-viewer",
            ExportFormat::Latex => "export-format-latex",
            ExportFormat::Epd => "export-format-epd",
            ExportFormat::List => "export-format-list",
            ExportFormat::Csv => "export-format-csv",
        }
    }
}

impl ImageFormat {
    /// The extension of the file, without the dot.
    pub fn extension(self) -> &'static str {
        self.name()
    }
}

impl CsvSeparator {
    /// The character.
    pub fn char(self) -> char {
        match self {
            CsvSeparator::Comma => ',',
            CsvSeparator::Semicolon => ';',
        }
    }
}

/// Largest value of `diagram_px`.
pub const MAX_DIAGRAM_PX: u16 = 1000;
/// Largest games-per-volume of the viewer (the `mychess-db` limit).
pub const MAX_PER_FILE: u32 = 5000;
/// Longest list format, in characters.
pub const MAX_LIST_FORMAT_CHARS: usize = 200;
/// Longest document template, in bytes.
pub const MAX_TEMPLATE_BYTES: usize = 64 * 1024;
/// The default format of the game list (Scid's `glexportDefault`).
pub const DEFAULT_LIST_FORMAT: &str = mychess_db::export::game_list::DEFAULT_LIST_FORMAT;

/// Options of the HTML export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HtmlPrefs {
    /// Write the comments.
    pub comments: bool,
    /// Write the variations.
    pub variations: bool,
    /// NAGs as symbols (`!`) instead of numbers (`$1`).
    pub symbols: bool,
    /// Width of the diagrams, `0` for none, up to [`MAX_DIAGRAM_PX`].
    pub diagram_px: u16,
}

impl Default for HtmlPrefs {
    fn default() -> HtmlPrefs {
        HtmlPrefs {
            comments: true,
            variations: true,
            symbols: false,
            diagram_px: 200,
        }
    }
}

/// Options of the HTML viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewerPrefs {
    /// Games per volume, `1..=5000`.
    pub per_file: u32,
    /// Also write `X.pgn`.
    pub include_pgn: bool,
    /// Open the first volume in the browser at the end (D3708).
    pub open_after: bool,
}

impl Default for ViewerPrefs {
    fn default() -> ViewerPrefs {
        ViewerPrefs {
            per_file: 500,
            include_pgn: true,
            open_after: false,
        }
    }
}

/// Options of the LaTeX export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LatexPrefs {
    /// Write the comments.
    pub comments: bool,
    /// Write the variations.
    pub variations: bool,
    /// Draw the diagrams from Black's side (D3712).
    pub flip: bool,
    /// Turn null moves into comments.
    pub convert_null_moves: bool,
}

impl Default for LatexPrefs {
    fn default() -> LatexPrefs {
        LatexPrefs {
            comments: true,
            variations: true,
            flip: false,
            convert_null_moves: true,
        }
    }
}

/// Options of the EPD export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpdPrefs {
    /// Write each position once.
    pub dedupe: bool,
    /// Use the final position of every game.
    pub final_position: bool,
    /// Add `hmvc` and `fmvn`.
    pub hmvc_fmvn: bool,
    /// Add an `id`.
    pub id: bool,
    /// Add `c0` with the number of the game.
    pub game_number: bool,
}

impl Default for EpdPrefs {
    fn default() -> EpdPrefs {
        EpdPrefs {
            dedupe: true,
            final_position: false,
            hmvc_fmvn: false,
            id: false,
            game_number: false,
        }
    }
}

/// Options of the CSV export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CsvPrefs {
    /// A header row.
    pub header: bool,
    /// The separator.
    pub separator: CsvSeparator,
    /// A UTF-8 byte order mark.
    pub bom: bool,
}

impl Default for CsvPrefs {
    fn default() -> CsvPrefs {
        CsvPrefs {
            header: true,
            separator: CsvSeparator::Comma,
            bom: false,
        }
    }
}

/// Options of the board image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImagePrefs {
    /// PNG or SVG.
    pub format: ImageFormat,
    /// Side of the board in pixels, `64..=4096`.
    pub size_px: u32,
    /// Coordinates inside the squares.
    pub coordinates: bool,
    /// Highlight the last move.
    pub last_move: bool,
    /// Draw the arrows and marks of the comment.
    pub marks: bool,
    /// A transparent background (PNG).
    pub transparent: bool,
    /// Margin in pixels, `0..=512`.
    pub margin_px: u32,
}

impl Default for ImagePrefs {
    fn default() -> ImagePrefs {
        ImagePrefs {
            format: ImageFormat::Png,
            size_px: 512,
            coordinates: true,
            last_move: true,
            marks: true,
            transparent: false,
            margin_px: 0,
        }
    }
}

fn is_default_html_start(text: &String) -> bool {
    text == mychess_pgn::export::default_html_start()
}
fn is_default_html_end(text: &String) -> bool {
    text == mychess_pgn::export::default_html_end()
}
fn is_default_latex_start(text: &String) -> bool {
    text == mychess_pgn::export::default_latex_start()
}
fn is_default_latex_end(text: &String) -> bool {
    text == mychess_pgn::export::default_latex_end()
}

/// Everything "Export games..." and the board image remember, in `[export]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportPrefs {
    /// The format the dialog proposes.
    pub format: ExportFormat,
    /// HTML options (`[export.html]`).
    pub html: HtmlPrefs,
    /// Viewer options.
    pub viewer: ViewerPrefs,
    /// LaTeX options.
    pub latex: LatexPrefs,
    /// EPD options.
    pub epd: EpdPrefs,
    /// The format string of the game list.
    pub list_format: String,
    /// CSV options.
    pub csv: CsvPrefs,
    /// Image options.
    pub image: ImagePrefs,
    /// The text before the games of an HTML file (the document template,
    /// D3715); only written to the file when it differs from the default.
    #[serde(skip_serializing_if = "is_default_html_start")]
    pub html_start: String,
    /// The text after the games of an HTML file.
    #[serde(skip_serializing_if = "is_default_html_end")]
    pub html_end: String,
    /// The preamble of a LaTeX file.
    #[serde(skip_serializing_if = "is_default_latex_start")]
    pub latex_start: String,
    /// The end of a LaTeX file.
    #[serde(skip_serializing_if = "is_default_latex_end")]
    pub latex_end: String,
}

impl Default for ExportPrefs {
    fn default() -> ExportPrefs {
        ExportPrefs {
            format: ExportFormat::Pgn,
            html: HtmlPrefs::default(),
            viewer: ViewerPrefs::default(),
            latex: LatexPrefs::default(),
            epd: EpdPrefs::default(),
            list_format: DEFAULT_LIST_FORMAT.to_owned(),
            csv: CsvPrefs::default(),
            image: ImagePrefs::default(),
            html_start: mychess_pgn::export::default_html_start().to_owned(),
            html_end: mychess_pgn::export::default_html_end().to_owned(),
            latex_start: mychess_pgn::export::default_latex_start().to_owned(),
            latex_end: mychess_pgn::export::default_latex_end().to_owned(),
        }
    }
}

impl ExportPrefs {
    /// The value with every field out of its range back at the default.
    #[must_use]
    pub fn sanitized(mut self) -> ExportPrefs {
        let d = ExportPrefs::default();
        if self.html.diagram_px > MAX_DIAGRAM_PX {
            self.html.diagram_px = d.html.diagram_px;
        }
        if !(1..=MAX_PER_FILE).contains(&self.viewer.per_file) {
            self.viewer.per_file = d.viewer.per_file;
        }
        if !SIZE_RANGE.contains(&self.image.size_px) {
            self.image.size_px = d.image.size_px;
        }
        if self.image.margin_px > MAX_MARGIN_PX {
            self.image.margin_px = d.image.margin_px;
        }
        if self.list_format.chars().count() > MAX_LIST_FORMAT_CHARS
            || self.list_format.chars().any(char::is_control)
        {
            self.list_format = d.list_format;
        }
        for (text, default) in [
            (&mut self.html_start, d.html_start),
            (&mut self.html_end, d.html_end),
            (&mut self.latex_start, d.latex_start),
            (&mut self.latex_end, d.latex_end),
        ] {
            if text.len() > MAX_TEMPLATE_BYTES {
                *text = default;
            }
        }
        self
    }
}
