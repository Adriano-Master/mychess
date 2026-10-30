//! The blocking work of the export of games (spec M3-9, D2005-D2016).
//!
//! `perform` runs [`export_run`] on `spawn_blocking`; `update` never calls
//! it.

use std::path::{Path, PathBuf};

use mychess_db::si4::GameFlags;
use mychess_db::{
    CancelToken, ExportError, ExportItem, ExportOptions, ExportSkipReason, GameSource, export_pgn,
};
use mychess_pgn::write::Encoding;

use super::{
    ExportList, ExportSkipCause, GamesExportJob, GamesExportOutcome, GamesExportReport,
    MAX_SKIPPED_LINES,
};
use crate::bases::{BaseError, OpenControl, SharedSource};
use crate::error::{FileError, FileErrorKind};
use crate::files::sanitize_file_name;

/// Entries looked at under one read lock, and between two looks at the
/// cancellation.
const SCAN_CHUNK: usize = 4096;

/// `path` with the name made safe for Linux, Windows and macOS and `.pgn` at
/// the end (D2012). Only the name changes, never the directory. `None` when
/// the name is not valid Unicode: it cannot be fixed without changing it
/// silently, so the export refuses it (D2022).
pub(crate) fn fix_destination(path: &Path) -> Option<PathBuf> {
    let Some(name) = path.file_name() else {
        return Some(path.to_path_buf());
    };
    let name = name.to_str()?.to_owned();
    let stem = match name.len().checked_sub(4).and_then(|at| name.get(at..)) {
        Some(tail) if tail.eq_ignore_ascii_case(".pgn") => {
            name.get(..name.len() - 4).unwrap_or(&name)
        }
        _ => name.as_str(),
    };
    let fixed = format!("{}.pgn", sanitize_file_name(stem));
    Some(path.with_file_name(fixed))
}

fn file_error(kind: FileErrorKind, path: &Path) -> BaseError {
    BaseError::File(FileError::new(kind, Some(path.to_path_buf())))
}

/// `path` made canonical (links resolved) when it exists; otherwise its
/// directory made canonical with the file name put back. In lower case: the
/// case never tells two files apart (D2013).
fn identity(path: &Path) -> Option<String> {
    if let Ok(real) = std::fs::canonicalize(path) {
        return Some(real.to_string_lossy().to_lowercase());
    }
    let name = path.file_name()?;
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let dir = std::fs::canonicalize(parent).ok()?;
    Some(dir.join(name).to_string_lossy().to_lowercase())
}

/// Whether `dest` is one of the `files` of the open databases (D2013).
///
/// The comparison is in lower case on every platform, so on Linux `A.pgn`
/// and `a.pgn` count as the same file: a false positive that only refuses
/// a name, never overwrites one (D2174).
pub(crate) fn is_protected(dest: &Path, files: &[PathBuf]) -> bool {
    let Some(wanted) = identity(dest) else {
        return false;
    };
    files.iter().any(|file| {
        // A file that does not exist any more cannot be overwritten by
        // mistake, but a path that cannot be resolved is compared as given.
        identity(file).is_some_and(|id| id == wanted)
            || file.to_string_lossy().to_lowercase() == dest.to_string_lossy().to_lowercase()
    })
}

fn to_base_error(error: ExportError) -> BaseError {
    match error {
        ExportError::Io { path, source } => BaseError::File(FileError::from_io(&source, path)),
        ExportError::NoFileName { path } => file_error(FileErrorKind::NotFound, &path),
        _ => BaseError::Internal,
    }
}

/// The numbers of the games to write, ascending, and how many were left out
/// by the `T` flag. The index is read in chunks, looking at the
/// cancellation between them.
pub(crate) fn collect_numbers(
    source: &SharedSource,
    list: &ExportList,
    only_tactics: bool,
    control: &OpenControl,
) -> Result<(Vec<u32>, u64), BaseError> {
    let candidates: Vec<u32> = match list {
        ExportList::Filter(filter) => filter.iter_games().collect(),
        ExportList::Numbers(numbers) => {
            let mut numbers = numbers.clone();
            numbers.sort_unstable();
            numbers.dedup();
            numbers
        }
        ExportList::Current(_) => return Ok((Vec::new(), 0)),
    };
    if !only_tactics {
        return Ok((candidates, 0));
    }
    let mut kept = Vec::new();
    let mut excluded = 0u64;
    for chunk in candidates.chunks(SCAN_CHUNK) {
        if control.is_cancelled() {
            return Err(BaseError::Cancelled);
        }
        let db = source.read();
        for &number in chunk {
            let flagged = GameSource::entry(&*db, number)
                .is_ok_and(|entry| entry.flags.contains(GameFlags::TACTICS));
            if flagged {
                kept.push(number);
            } else {
                excluded += 1;
            }
        }
    }
    Ok((kept, excluded))
}

fn skip_cause(reason: &ExportSkipReason) -> ExportSkipCause {
    match reason {
        ExportSkipReason::Unreadable(_) => ExportSkipCause::Unreadable,
        _ => ExportSkipCause::Unwritable,
    }
}

/// Writes the games of `job`.
///
/// A new file goes through a temporary in the same directory and is renamed
/// at the end; stopping the job throws it away and leaves the destination as
/// it was (D2010). An append keeps what was added (D2011).
///
/// # Errors
///
/// [`BaseError::Cancelled`] when stopped before anything was written;
/// [`BaseError::ExportOverBase`] when the destination is the file of an open
/// database; [`BaseError::File`] when the destination cannot be written
/// (permission, missing directory, disk full, a file that may not be
/// replaced).
pub(crate) fn export_run(
    job: &GamesExportJob,
    control: &OpenControl,
) -> Result<GamesExportOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let dest = job.dest.as_path();
    if is_protected(dest, &job.protected) {
        return Err(BaseError::ExportOverBase);
    }
    let exists = dest.exists();
    let append = job.append && exists;
    if exists && !job.confirmed {
        // The dialog confirmed another name: never replace or add to this
        // one (D2012).
        return Err(file_error(FileErrorKind::AlreadyExists, dest));
    }
    if exists && !append {
        // Renaming over a read-only file works on Unix and fails on
        // Windows: refuse it everywhere (D2014).
        if std::fs::metadata(dest).is_ok_and(|m| m.permissions().readonly()) {
            return Err(file_error(FileErrorKind::PermissionDenied, dest));
        }
    }
    let (numbers, excluded) = collect_numbers(&job.source, &job.list, job.only_tactics, control)?;
    let mut report = GamesExportReport {
        excluded,
        appended: append,
        ..GamesExportReport::default()
    };
    let total = match &job.list {
        ExportList::Current(_) => 1,
        _ => u64::try_from(numbers.len()).unwrap_or(u64::MAX),
    };
    let mut progress = control.reporter();
    progress(0, total);
    if total == 0 {
        return Ok(GamesExportOutcome {
            path: dest.to_path_buf(),
            report,
        });
    }
    let mut options = ExportOptions::default();
    options.pgn = job.write.clone();
    options.encoding = Encoding::Utf8;
    options.append = append;
    options.total = Some(total);
    options.discard_on_cancel = true;
    options.discard_if_empty = true;
    let token: CancelToken = control.token();
    let done = if let ExportList::Current(game) = &job.list {
        export_pgn(
            [ExportItem::Game(Box::new(game.to_game()))],
            dest,
            &options,
            &mut progress,
            &token,
        )
    } else {
        let source = job.source.clone();
        // The write lock is taken for one game at a time, so the list
        // can be drawn while the job runs (D2006).
        let items = numbers.into_iter().map(move |n| {
            let mut db = source.lock();
            ExportItem::stored(&mut *db, n)
        });
        export_pgn(items, dest, &options, &mut progress, &token)
    }
    .map_err(to_base_error)?;
    report.written = done.written;
    report.interrupted = done.interrupted;
    report.skipped = u64::try_from(done.skipped.len()).unwrap_or(u64::MAX);
    report.lines = done
        .skipped
        .iter()
        .take(MAX_SKIPPED_LINES)
        .map(|(number, reason)| (*number, skip_cause(reason)))
        .collect();
    Ok(GamesExportOutcome {
        path: done.path,
        report,
    })
}
