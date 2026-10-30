// Scid: src/optable.cpp `PrintText`/`PrintHTML`/`PrintLaTeX`/`PrintNotes`/
// `OpLine::PrintSummary` 365-481 and 1027-1437; tcl/tools/optable.tcl
// `::optable::report` 790-1140. Copyright (c) 2001-2003 Shane Hudson. Scid
// is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The text, HTML and LaTeX documents of an opening report (decisions D3617
//! and D3628). The document is first built as a list of [`Block`]s (what the
//! Scid's `::optable::report` appends), then written in each format. The
//! output is always UTF-8 with `\n` and never contains `NUL`.

mod blocks;
mod emit;
mod player;
mod theory;

use super::labels::ReportLabels;
use super::model::OpeningReport;
use super::options::ReportOptions;
use super::player::{PlayerLabels, PlayerReport, PlayerReportOptions};

/// Which part of the report is written (D3616).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReportPart {
    /// The whole report, with the theory table when it is on.
    Full,
    /// The report without the theory table.
    Compact,
    /// Only the theory table.
    TableOnly,
}

/// The format of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fmt {
    Text,
    Html,
    Latex,
}

/// The report as plain text (the Scid's `text` format).
pub fn render_text(
    report: &OpeningReport,
    options: &ReportOptions,
    labels: &ReportLabels,
    part: ReportPart,
) -> String {
    render(Fmt::Text, report, options, labels, part)
}

/// The report as a complete HTML document. Every name is escaped.
pub fn render_html(
    report: &OpeningReport,
    options: &ReportOptions,
    labels: &ReportLabels,
    part: ReportPart,
) -> String {
    render(Fmt::Html, report, options, labels, part)
}

/// The report as a complete LaTeX document (for `xskak`). Every name is
/// escaped.
pub fn render_latex(
    report: &OpeningReport,
    options: &ReportOptions,
    labels: &ReportLabels,
    part: ReportPart,
) -> String {
    render(Fmt::Latex, report, options, labels, part)
}

fn render(
    fmt: Fmt,
    report: &OpeningReport,
    options: &ReportOptions,
    labels: &ReportLabels,
    part: ReportPart,
) -> String {
    let opts = options.normalized();
    let doc = blocks::build(report, &opts, labels, part);
    emit::document(fmt, &doc, labels)
}

/// The line to the reference position, as the header and the theory table
/// need it (shared by the opening report and the player report).
#[derive(Debug, Clone, Copy)]
pub(crate) struct StemView<'a> {
    /// The moves in SAN (the last ones, see `MAX_STEM_PLIES`).
    pub stem: &'a [String],
    /// Half moves of the start of the line that are not in `stem`.
    pub omitted: usize,
    /// The move left out of the theory table, if any.
    pub excluded: Option<&'a str>,
}

impl StemView<'_> {
    /// Half moves of the line, the omitted ones included.
    pub(super) fn plies(&self) -> usize {
        self.stem.len() + self.omitted
    }

    /// The number of the first move of the table (`(plies + 2) / 2`).
    pub(super) fn first_move_number(&self) -> usize {
        usize::midpoint(self.plies(), 2)
    }
}

/// The player report in `fmt` (the writers are the ones of the opening
/// report).
pub(crate) fn render_player(
    fmt: Fmt,
    report: &PlayerReport,
    options: &PlayerReportOptions,
    labels: &PlayerLabels,
    part: ReportPart,
) -> String {
    let opts = options.normalized();
    let doc = player::build(report, &opts, labels, part);
    emit::document(fmt, &doc, &labels.base)
}

/// A piece of the document, independent of the format.
#[derive(Debug, Clone)]
pub(crate) enum Block {
    /// The title of the document (the line, or "Opening Report").
    Title { text: String, line: Vec<String> },
    /// The board of the reference position.
    Board { fen: String },
    /// `label: value` lines of the header.
    Header(Vec<(String, String)>),
    /// A numbered section.
    Section { number: u32, text: String },
    /// A numbered subsection.
    Subsection {
        section: u32,
        number: u32,
        text: String,
    },
    /// A line of text.
    Paragraph(String),
    /// A table of cells.
    Grid(Grid),
    /// A list of games.
    Games(Vec<GameItem>),
    /// The theory table.
    Theory(Box<TheoryBlock>),
    /// The notes of the theory table.
    Notes(Vec<NoteBlock>),
}

/// The alignment of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Right,
}

/// A table of plain cells; the writers escape them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Grid {
    pub aligns: Vec<Align>,
    pub header: Vec<Vec<String>>,
    pub rows: Vec<Vec<String>>,
}

/// A game of a list (oldest, newest, best, shortest).
#[derive(Debug, Clone)]
pub(crate) struct GameItem {
    pub rank: usize,
    pub summary: Summary,
    pub full_date: bool,
    pub note: u32,
}

/// What `OpLine::PrintSummary` writes about a game.
#[derive(Debug, Clone)]
pub(crate) struct Summary {
    pub result: mychess_core::GameResult,
    /// Full moves, shown when `show_moves` or the game goes on after the
    /// line.
    pub moves: u32,
    pub show_moves: bool,
    pub white: String,
    pub black: String,
    pub white_elo: u16,
    pub black_elo: u16,
    pub site: String,
    pub year: String,
    pub date: String,
}

/// The theory table.
#[derive(Debug, Clone)]
pub(crate) struct TheoryBlock {
    /// The line to the position, with the excluded move.
    pub stem: String,
    pub heading: String,
    pub first_number: usize,
    pub rows: Vec<TheoryRow>,
    pub comment: String,
}

/// A row: the cells (compact SAN with note markers) and the totals.
#[derive(Debug, Clone)]
pub(crate) struct TheoryRow {
    pub number: usize,
    /// 16 cells: White, Black, White, ...
    pub cells: Vec<TheoryCell>,
    pub count: u32,
    pub score: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TheoryCell {
    pub text: String,
    pub note: u32,
}

/// A note with its pieces already written as text parts.
#[derive(Debug, Clone)]
pub(crate) struct NoteBlock {
    pub number: u32,
    pub entries: Vec<NoteText>,
}

/// One game of a note: how it is joined to the previous one and its text.
#[derive(Debug, Clone)]
pub(crate) struct NoteText {
    pub join: Join,
    /// The numbered moves from the start of the note text; the first one is
    /// bold in HTML and LaTeX.
    pub first_move: String,
    pub other_moves: String,
    pub summary: Summary,
}

/// How a game of a note follows the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Join {
    First,
    /// A new paragraph (the game continues the same variation).
    Paragraph,
    /// `;` and the next game.
    Next,
}
