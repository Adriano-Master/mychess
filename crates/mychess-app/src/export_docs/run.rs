//! The blocking work of the extra formats (spec M7-4, slice b).
//!
//! `perform` runs [`docs_run`] on `spawn_blocking`; `update` never calls it.

use std::path::{Path, PathBuf};

use mychess_db::export::epd::EpdItem;
use mychess_db::export::game_list::GameListOptions;
use mychess_db::export::names::{stem_and_dir, volume_file_name};
use mychess_db::export::viewer::ViewerAssets;
use mychess_db::{
    CancelToken, CsvOptions, EpdExportOptions, ExportError, ExportItem, ExportSkipReason,
    HtmlExportOptions, LatexExportOptions, ViewerOptions, export_epd, export_game_csv,
    export_game_list, export_html, export_latex, viewer_volumes,
};
use mychess_pgn::export::{HtmlOptions, LatexOptions, NoDiagrams};

use super::{ConfirmKind, ConfirmNeeded, ExportDocsJob, ExportFormat, ExportProblem};
use crate::bases::{BaseError, OpenControl};
use crate::board_image::{BoardDiagrams, piece_symbols};
use crate::error::{FileError, FileErrorKind};
use crate::files::sanitize_file_name;
use crate::games_export::{
    ExportList, ExportSkipCause, GamesExportOutcome, GamesExportReport, MAX_SKIPPED_LINES,
    collect_numbers, is_protected,
};

/// The viewer script and stylesheet (placeholders until slice d).
const VIEWER_JS: &str = include_str!("../../../../assets/export/viewer.js");
const VIEWER_CSS: &str = include_str!("../../../../assets/export/viewer.css");

/// `path` with the name made safe for Linux, Windows and macOS and the
/// extension of `format` at the end (D2012, D3753). Only the name changes,
/// never the directory. `None` when the name is not valid Unicode.
pub(crate) fn fix_destination_ext(
    path: &Path,
    accepted: &[&str],
    extension: &str,
) -> Option<PathBuf> {
    let Some(name) = path.file_name() else {
        return Some(path.to_path_buf());
    };
    let name = name.to_str()?;
    let stem = accepted
        .iter()
        .find_map(|ext| {
            let cut = name.len().checked_sub(ext.len() + 1)?;
            let tail = name.get(cut..)?;
            let (dot, tail) = tail.split_at_checked(1)?;
            (dot == "." && tail.eq_ignore_ascii_case(ext)).then(|| name.get(..cut))?
        })
        .unwrap_or(name);
    Some(path.with_file_name(format!("{}.{extension}", sanitize_file_name(stem))))
}

fn file_error(kind: FileErrorKind, path: &Path) -> BaseError {
    BaseError::File(FileError::new(kind, Some(path.to_path_buf())))
}

fn skip_cause(reason: &ExportSkipReason) -> ExportSkipCause {
    match reason {
        ExportSkipReason::Unreadable(_) => ExportSkipCause::Unreadable,
        _ => ExportSkipCause::Unwritable,
    }
}

fn fill_skips(report: &mut GamesExportReport, skipped: &[(u32, ExportSkipReason)]) {
    report.skipped = u64::try_from(skipped.len()).unwrap_or(u64::MAX);
    report.lines = skipped
        .iter()
        .take(MAX_SKIPPED_LINES)
        .map(|(number, reason)| (*number, skip_cause(reason)))
        .collect();
}

/// The `.pgn` that goes with the first volume of the viewer.
///
/// Uses the sanitizing of the writer, so that both name the same file.
fn viewer_pgn_path(dest: &Path) -> PathBuf {
    match stem_and_dir(dest) {
        Some((stem, dir)) => dir.join(format!("{stem}.pgn")),
        None => dest.with_extension("pgn"),
    }
}

/// The volumes the viewer would write that are already there: `X.html`
/// (when the writer's name differs from `dest`) and `X_2.html`, `X_3.html`,
/// ... in a row (D4310).
fn viewer_volume_files(dest: &Path) -> Vec<PathBuf> {
    let Some((stem, dir)) = stem_and_dir(dest) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let first = dir.join(volume_file_name(&stem, 1));
    if first != dest && exists(&first) && !same_file(&first, dest) {
        found.push(first);
    }
    // A row of existing volumes; the cap keeps a hostile folder cheap.
    for n in 2..=MAX_VOLUMES_LOOKED_AT {
        let path = dir.join(volume_file_name(&stem, n));
        if !exists(&path) {
            break;
        }
        found.push(path);
    }
    found
}

/// Most volumes `X_n.html` looked for before a replacement question.
const MAX_VOLUMES_LOOKED_AT: u32 = 5000;

/// Whether both paths name the same existing file. On case-insensitive
/// file systems (Windows, macOS) `Foo.HTML` and `Foo.html` canonicalize to
/// one path; on Linux they stay two files.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// What needs the user's word, if anything (D3750).
fn confirmation(job: &ExportDocsJob, append: bool) -> Option<ConfirmNeeded> {
    let dest = job.dest.as_path();
    let mut files = Vec::new();
    let kind = if append && exists(dest) {
        // Appending is never confirmed by the file dialog.
        files.push(dest.to_path_buf());
        ConfirmKind::Append
    } else {
        if exists(dest) && !job.dialog_confirmed {
            files.push(dest.to_path_buf());
        }
        if job.format == ExportFormat::Viewer {
            files.extend(viewer_volume_files(dest));
            if job.options.viewer.include_pgn {
                let pgn = viewer_pgn_path(dest);
                if exists(&pgn) {
                    files.push(pgn);
                }
            }
        }
        ConfirmKind::Replace
    };
    (!files.is_empty()).then_some(ConfirmNeeded { kind, files })
}

/// Letters that make the ids of the diagrams of an appended run differ from
/// those already in the file (D3752).
fn append_salt() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("a{seconds:x}")
}

fn problem(problem: ExportProblem, dest: &Path) -> GamesExportOutcome {
    GamesExportOutcome {
        path: dest.to_path_buf(),
        report: GamesExportReport {
            problem: Some(problem),
            ..GamesExportReport::default()
        },
    }
}

/// Maps the error of a writer: files are errors, the rest is a report.
fn from_export_error(error: ExportError, dest: &Path) -> Result<GamesExportOutcome, BaseError> {
    let outcome = |report: GamesExportReport| GamesExportOutcome {
        path: dest.to_path_buf(),
        report,
    };
    match error {
        ExportError::Io { path, source } => Err(BaseError::File(FileError::from_io(&source, path))),
        ExportError::NoFileName { path } => Err(file_error(FileErrorKind::NotFound, &path)),
        ExportError::NoAppendMarker { .. } => Ok(problem(ExportProblem::NoAppendMarker, dest)),
        ExportError::NotUtf8 { .. } => Ok(problem(ExportProblem::NotUtf8, dest)),
        ExportError::EmptyFormat => Ok(problem(ExportProblem::EmptyFormat, dest)),
        ExportError::InvalidOptions(_) => Ok(problem(ExportProblem::InvalidOptions, dest)),
        ExportError::NothingToExport => Ok(outcome(GamesExportReport::default())),
        ExportError::AllSkipped { skipped, first, .. } => {
            let mut report = GamesExportReport {
                skipped,
                ..GamesExportReport::default()
            };
            report.lines = first
                .iter()
                .map(|(number, reason)| (*number, skip_cause(reason)))
                .collect();
            Ok(outcome(report))
        }
        _ => Err(BaseError::Internal),
    }
}

/// A progress reporter that never goes back (the control keeps the last
/// value).
fn monotonic(control: &OpenControl) -> impl FnMut(u64, u64) + use<> {
    let mut report = control.reporter();
    let mut last = 0u64;
    move |done, total| {
        last = last.max(done);
        report(last, total.max(last));
    }
}

/// Writes the games of `job` in the format it asks for.
///
/// Every file is written to a temporary in its directory and renamed at the
/// end; stopping the job throws the new output away (an appended EPD, list or
/// CSV file is rewritten the same way). Nothing is written when the file
/// needs the user's word: the report says so (`confirm`).
///
/// # Errors
///
/// [`BaseError::Cancelled`] when stopped before anything was written;
/// [`BaseError::ExportOverBase`] when a destination is the file of an open
/// database; [`BaseError::File`] when a file cannot be written.
pub(crate) fn docs_run(
    job: &ExportDocsJob,
    control: &OpenControl,
) -> Result<GamesExportOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let dest = job.dest.as_path();
    let viewer = job.format == ExportFormat::Viewer;
    if is_protected(dest, &job.protected)
        || (viewer
            && job.options.viewer.include_pgn
            && is_protected(&viewer_pgn_path(dest), &job.protected))
    {
        return Err(BaseError::ExportOverBase);
    }
    // A file that is not there yet is simply created (as the PGN export does).
    let append = job.append && job.format.can_append() && exists(dest);
    if !job.confirmed
        && let Some(confirm) = confirmation(job, append)
    {
        return Ok(GamesExportOutcome {
            path: dest.to_path_buf(),
            report: GamesExportReport {
                confirm: Some(confirm),
                ..GamesExportReport::default()
            },
        });
    }
    if !append && exists(dest) && std::fs::metadata(dest).is_ok_and(|m| m.permissions().readonly())
    {
        return Err(file_error(FileErrorKind::PermissionDenied, dest));
    }
    if viewer && job.assets.is_none() {
        return Ok(problem(ExportProblem::AssetsMissing, dest));
    }
    if job.format.needs_database() && matches!(job.list, ExportList::Current(_)) {
        return Ok(problem(ExportProblem::InvalidOptions, dest));
    }
    let (numbers, excluded) = collect_numbers(&job.source, &job.list, job.only_tactics, control)?;
    let total = match &job.list {
        ExportList::Current(_) => 1,
        _ => u64::try_from(numbers.len()).unwrap_or(u64::MAX),
    };
    let mut report = GamesExportReport {
        excluded,
        appended: append,
        ..GamesExportReport::default()
    };
    let mut progress = monotonic(control);
    progress(0, total);
    if total == 0 {
        return Ok(GamesExportOutcome {
            path: dest.to_path_buf(),
            report,
        });
    }
    let token = control.token();
    let result = write_format(
        job,
        append,
        numbers,
        total,
        &mut progress,
        &token,
        &mut report,
    );
    match result {
        Ok(path) => Ok(GamesExportOutcome { path, report }),
        Err(error) => from_export_error(error, dest).map(|mut outcome| {
            outcome.report.excluded = excluded;
            outcome
        }),
    }
}

/// The items of the job, one game at a time under the write lock.
fn items(job: &ExportDocsJob, numbers: Vec<u32>) -> Box<dyn Iterator<Item = ExportItem> + Send> {
    if let ExportList::Current(game) = &job.list {
        return Box::new(std::iter::once(ExportItem::Game(Box::new(game.to_game()))));
    }
    let source = job.source.clone();
    Box::new(numbers.into_iter().map(move |n| {
        let mut db = source.lock();
        ExportItem::stored(&mut *db, n)
    }))
}

/// Runs `write` on a reserved name next to `dest` and moves the result over
/// `dest` only if it holds a game, so a job that writes no game (all skipped,
/// or none) leaves the old file alone (D3755). The writers of HTML and LaTeX
/// would otherwise leave an empty document behind.
fn write_via_part(
    dest: &Path,
    extension: &str,
    write: impl FnOnce(&Path) -> Result<mychess_db::ExportReport, ExportError>,
) -> Result<mychess_db::ExportReport, ExportError> {
    let dir = match dest.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let io = |source: std::io::Error| ExportError::Io {
        path: dest.to_path_buf(),
        source,
    };
    let part = tempfile::Builder::new()
        .prefix(".mychess-export-")
        .suffix(&format!(".{extension}"))
        .tempfile_in(dir)
        .map_err(io)?
        .into_temp_path();
    let mut report = write(&part)?;
    if report.written == 0 {
        // The temporary goes away when `part` is dropped.
        report.path = dest.to_path_buf();
        return Ok(report);
    }
    part.persist(dest).map_err(|e| io(e.error))?;
    report.path = dest.to_path_buf();
    Ok(report)
}

#[allow(clippy::too_many_lines)] // one arm per format
fn write_format(
    job: &ExportDocsJob,
    append: bool,
    numbers: Vec<u32>,
    total: u64,
    progress: &mut dyn mychess_db::ProgressSink,
    token: &CancelToken,
    report: &mut GamesExportReport,
) -> Result<PathBuf, ExportError> {
    let o = &job.options;
    let dest = job.dest.as_path();
    match job.format {
        ExportFormat::Html => {
            let assets = job.assets.clone();
            let options = HtmlExportOptions {
                html: HtmlOptions {
                    comments: o.html.comments,
                    variations: o.html.variations,
                    symbols: o.html.symbols,
                    // No drawings from the host: no diagrams.
                    diagram_px: if assets.is_some() {
                        o.html.diagram_px
                    } else {
                        0
                    },
                    indent_variations: true,
                    start: o.html_start.clone(),
                    end: o.html_end.clone(),
                },
                append,
                total: Some(total),
                discard_on_cancel: true,
            };
            let salt = if append { append_salt() } else { String::new() };
            let mut diagrams =
                assets.map(|a| BoardDiagrams::new(a.pieces.clone(), a.palette.clone(), &salt));
            let games = items(job, numbers);
            let run = |to: &Path| match diagrams.as_mut() {
                Some(d) => export_html(games, to, &options, d, progress, token),
                None => export_html(games, to, &options, &mut NoDiagrams, progress, token),
            };
            let done = if append {
                run(dest)?
            } else {
                write_via_part(dest, "html", run)?
            };
            report.written = done.written;
            report.interrupted = done.interrupted;
            fill_skips(report, &done.skipped);
            Ok(done.path)
        }
        ExportFormat::Latex => {
            let options = LatexExportOptions {
                latex: LatexOptions {
                    comments: o.latex.comments,
                    variations: o.latex.variations,
                    flip: o.latex.flip,
                    convert_null_moves: o.latex.convert_null_moves,
                    start: o.latex_start.clone(),
                    end: o.latex_end.clone(),
                },
                append,
                total: Some(total),
                discard_on_cancel: true,
            };
            let games = items(job, numbers);
            let run = |to: &Path| export_latex(games, to, &options, progress, token);
            let done = if append {
                run(dest)?
            } else {
                write_via_part(dest, "tex", run)?
            };
            report.written = done.written;
            report.interrupted = done.interrupted;
            fill_skips(report, &done.skipped);
            Ok(done.path)
        }
        ExportFormat::Epd => {
            let options = EpdExportOptions {
                dedupe: o.epd.dedupe,
                final_position: o.epd.final_position,
                hmvc_fmvn: o.epd.hmvc_fmvn,
                id: o.epd.id,
                game_number: o.epd.game_number,
                append,
                discard_on_cancel: true,
            };
            let source = job.source.clone();
            let list = job.list.clone();
            let current = matches!(list, ExportList::Current(_));
            let epd_items: Box<dyn Iterator<Item = EpdItem> + Send> = if current {
                Box::new(items(job, numbers).map(|item| EpdItem::new(item, None)))
            } else {
                Box::new(numbers.into_iter().map(move |n| {
                    let ply = match &list {
                        ExportList::Filter(filter) => {
                            Some(u32::from(filter.ply(n))).filter(|ply| *ply > 0)
                        }
                        _ => None,
                    };
                    let mut db = source.lock();
                    EpdItem::new(ExportItem::stored(&mut *db, n), ply)
                }))
            };
            let done = export_epd(epd_items, dest, &options, progress, token)?;
            report.written = done.written;
            report.interrupted = done.interrupted;
            report.repeated = done.repeated;
            report.not_selected = done.not_selected;
            fill_skips(report, &done.skipped);
            Ok(done.path)
        }
        ExportFormat::List => {
            let options = GameListOptions {
                format: o.list_format.clone(),
                append,
                ..GameListOptions::default()
            };
            // The read lock is held for the whole job: the lists only read
            // index entries (D3754).
            let db = job.source.read();
            let done = export_game_list(&*db, numbers, dest, &options, progress, token)?;
            report.written = done.written;
            report.interrupted = done.interrupted;
            fill_skips(report, &done.skipped);
            Ok(done.path)
        }
        ExportFormat::Csv => {
            let columns: Vec<_> = job.csv_columns.iter().map(|(c, _)| *c).collect();
            let header = o
                .csv
                .header
                .then(|| job.csv_columns.iter().map(|(_, h)| h.clone()).collect());
            let options = CsvOptions {
                columns,
                header,
                separator: o.csv.separator.char(),
                bom: o.csv.bom,
                append,
                ..CsvOptions::default()
            };
            let db = job.source.read();
            let done = export_game_csv(&*db, numbers, dest, &options, progress, token)?;
            report.written = done.written;
            report.interrupted = done.interrupted;
            fill_skips(report, &done.skipped);
            Ok(done.path)
        }
        ExportFormat::Viewer => {
            let Some(assets) = job.assets.clone() else {
                return Err(ExportError::InvalidOptions("no piece drawings"));
            };
            let options = ViewerOptions {
                per_file: usize::try_from(o.viewer.per_file).unwrap_or(usize::MAX),
                include_pgn: o.viewer.include_pgn,
                title: job.title.clone(),
                pieces_credit: assets.pieces_credit.clone(),
                assets: ViewerAssets {
                    css: VIEWER_CSS.to_owned(),
                    js: VIEWER_JS.to_owned(),
                    pieces_svg: piece_symbols(&assets.pieces, ""),
                },
                pgn: job.write.clone(),
                ..ViewerOptions::default()
            };
            let done = viewer_volumes(items(job, numbers), dest, &options, progress, token)?;
            report.written = done.written;
            report.interrupted = done.interrupted;
            report.volumes = u32::try_from(done.volumes.len()).unwrap_or(u32::MAX);
            fill_skips(report, &done.skipped);
            Ok(done
                .volumes
                .first()
                .cloned()
                .unwrap_or_else(|| dest.to_path_buf()))
        }
        // PGN is the job of `export_run`.
        ExportFormat::Pgn => Err(ExportError::InvalidOptions("not an extra format")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_volume_is_not_counted_twice_when_it_is_the_destination_itself() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("Foo.html");
        std::fs::write(&dest, "x").unwrap();
        // The destination is reported by `confirmation`, not here.
        assert!(viewer_volume_files(&dest).is_empty());
        // Same file under another spelling of the extension: on a
        // case-insensitive file system it is still the destination.
        let upper = dir.path().join("Foo.HTML");
        let found = viewer_volume_files(&upper);
        if same_file(&dest, &upper) {
            assert!(found.is_empty(), "{found:?}");
        } else {
            assert_eq!(found, vec![dest]);
        }
    }

    #[test]
    fn a_destination_without_a_stem_gets_a_real_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let (stem, _) = stem_and_dir(&dir.path().join(".html")).unwrap();
        assert!(!stem.is_empty());
        assert_eq!(volume_file_name(&stem, 1), "export.html");
        assert_eq!(
            viewer_pgn_path(&dir.path().join(".html")),
            dir.path().join("export.pgn")
        );
    }
}
