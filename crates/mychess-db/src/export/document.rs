// Behaviour derived from Scid vs PC 4.27, src/tkscid.cpp (sc_base_export,
// 1400-1626: HTML and LaTeX append mode, start and end text) and
// tcl/end.tcl (exportGames). Copyright (c) 1999-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Export of games to an HTML or a LaTeX file (decisions D3702, D3709 and
//! D3722 of `docs/specs/m7-4-exportacao.md`).
//!
//! [`export_html`] and [`export_latex`] take the games as an iterator of
//! [`ExportItem`]s, render each with the writers of `mychess_pgn::export` and
//! put them between the start and the end text of the options.
//!
//! - The file is written to a temporary in the same directory and renamed at
//!   the end; cancelling throws it away (by default) and leaves the old file.
//! - Append mode rewrites the file: everything before the **last** line that
//!   starts with `</body>` (HTML) or `\end{document}` (LaTeX), any letter
//!   case and leading white space ignored, is kept byte for byte, then the new
//!   games and the end text of the options follow (the old final lines are
//!   replaced by it). Without the marker the result is
//!   [`ExportError::NoAppendMarker`] and the file is not touched.
//! - A game that cannot be read or rendered is skipped and reported.

use std::path::{Path, PathBuf};

use mychess_pgn::export::{
    DiagramSource, HtmlOptions, LatexOptions, game_to_html_string, game_to_latex_string,
};

use super::text_file::{Append, Job, Step, run, with_extension_ci};
use super::{ExportError, ExportItem, ExportReport, ExportSkipReason};
use crate::progress::{CancelToken, ProgressSink};

/// Options of [`export_html`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlExportOptions {
    /// How each game is written, with the start and end text of the file.
    pub html: HtmlOptions,
    /// Add the games to an existing file (see the module documentation).
    pub append: bool,
    /// Number of games expected, for the progress reports (default: the
    /// `size_hint` of the iterator).
    pub total: Option<u64>,
    /// Throw the new output away when cancelled (default `true`); otherwise
    /// the games written so far are kept and the file is closed normally.
    pub discard_on_cancel: bool,
}

impl Default for HtmlExportOptions {
    fn default() -> HtmlExportOptions {
        HtmlExportOptions {
            html: HtmlOptions::default(),
            append: false,
            total: None,
            discard_on_cancel: true,
        }
    }
}

/// Options of [`export_latex`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexExportOptions {
    /// How each game is written, with the start and end text of the file.
    pub latex: LatexOptions,
    /// Add the games to an existing file (see the module documentation).
    pub append: bool,
    /// Number of games expected, for the progress reports.
    pub total: Option<u64>,
    /// Throw the new output away when cancelled (default `true`).
    pub discard_on_cancel: bool,
}

impl Default for LatexExportOptions {
    fn default() -> LatexExportOptions {
        LatexExportOptions {
            latex: LatexOptions::default(),
            append: false,
            total: None,
            discard_on_cancel: true,
        }
    }
}

fn total_of<I: Iterator>(total: Option<u64>, items: &I) -> u64 {
    total.unwrap_or_else(|| {
        let (low, high) = items.size_hint();
        u64::try_from(high.unwrap_or(low)).unwrap_or(0)
    })
}

fn into_report(path: PathBuf, r: super::text_file::JobResult) -> ExportReport {
    ExportReport {
        path,
        written: if r.discarded { 0 } else { r.written },
        skipped: r.skipped,
        interrupted: r.interrupted,
        unrepresentable: 0,
        elapsed: r.elapsed,
    }
}

fn game_of(item: ExportItem) -> (u32, Result<Box<mychess_core::Game>, ExportSkipReason>) {
    match item {
        ExportItem::Game(game) => (0, Ok(game)),
        ExportItem::Stored { number, game } => (
            number,
            game.map_err(|e| ExportSkipReason::Unreadable(e.to_string())),
        ),
    }
}

/// Writes `games` as an HTML document to `dest` (`.html` is added when the
/// name does not end in it, in any letter case). Diagrams come from
/// `diagrams`. The report has the final path; `written` is 0 when the output
/// was thrown away.
///
/// # Errors
///
/// [`ExportError::Io`] for files, [`ExportError::NoFileName`] for a
/// destination without a name, and in append mode
/// [`ExportError::NoAppendMarker`] / [`ExportError::NotUtf8`].
pub fn export_html<I>(
    games: I,
    dest: &Path,
    options: &HtmlExportOptions,
    diagrams: &mut dyn DiagramSource,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ExportReport, ExportError>
where
    I: IntoIterator<Item = ExportItem>,
{
    let games = games.into_iter();
    let dest = with_extension_ci(dest, "html");
    let job = Job {
        dest: &dest,
        start: &options.html.start,
        end: &options.html.end,
        append: if options.append {
            Append::BeforeMarker("</body>")
        } else {
            Append::No
        },
        discard_on_cancel: options.discard_on_cancel,
        discard_if_empty: false,
        total: total_of(options.total, &games),
    };
    let result = run(games, &job, "</body>", progress, cancel, |item, buf| {
        let (number, game) = game_of(item);
        match game.and_then(|g| {
            game_to_html_string(&g, &options.html, diagrams)
                .map_err(|e| ExportSkipReason::Unwritable(e.to_string()))
        }) {
            Ok(text) => {
                buf.push_str(&text);
                Step::Written
            }
            Err(reason) => Step::Skipped(number, reason),
        }
    })?;
    Ok(into_report(dest, result))
}

/// Writes `games` as a LaTeX document to `dest` (`.tex` is added when the
/// name does not end in it). See [`export_html`] for the rest.
///
/// # Errors
///
/// As [`export_html`], with `\end{document}` as the append marker.
pub fn export_latex<I>(
    games: I,
    dest: &Path,
    options: &LatexExportOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ExportReport, ExportError>
where
    I: IntoIterator<Item = ExportItem>,
{
    let games = games.into_iter();
    let dest = with_extension_ci(dest, "tex");
    let job = Job {
        dest: &dest,
        start: &options.latex.start,
        end: &options.latex.end,
        append: if options.append {
            Append::BeforeMarker("\\end{document}")
        } else {
            Append::No
        },
        discard_on_cancel: options.discard_on_cancel,
        discard_if_empty: false,
        total: total_of(options.total, &games),
    };
    let result = run(
        games,
        &job,
        "\\end{document}",
        progress,
        cancel,
        |item, buf| {
            let (number, game) = game_of(item);
            match game.and_then(|g| {
                game_to_latex_string(&g, &options.latex)
                    .map_err(|e| ExportSkipReason::Unwritable(e.to_string()))
            }) {
                Ok(text) => {
                    buf.push_str(&text);
                    Step::Written
                }
                Err(reason) => Step::Skipped(number, reason),
            }
        },
    )?;
    Ok(into_report(dest, result))
}
