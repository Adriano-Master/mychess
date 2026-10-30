//! The blocking work of the import of PGN files (spec M3-8, D1976-D1983).
//!
//! `perform` runs [`import_run`] on `spawn_blocking`; `update` never calls
//! it.

use std::path::Path;

use mychess_db::si4::{Si4Database, Si4Writer};
use mychess_db::{
    ImportError, ImportNoticeKind, ImportOptions, ImportReport as DbReport, ImportStop, SkipReason,
    import_pgn_file,
};
use mychess_pgn::read::Severity;

use super::{
    FileImportJob, FileImportOutcome, FileImportReport, FileImportTarget, ImportLine,
    MAX_REPORT_LINES,
};
use crate::bases::{BaseError, BaseSource, FileStamp, OpenControl, SharedSource};
use crate::error::FileError;
use crate::import::pgn_message_key;

/// The type written in the header of a database created here (the one of
/// the interface; the core writes 0).
const NEW_BASE_TYPE: u32 = 1;

/// Imports the files of `job` into its database, one after the other.
///
/// An existing database is checked first (the file must still be the one the
/// slot read, D1980); a new one is created and refused if it exists. The
/// writer is dropped before the database is read again, and the database is
/// read again whatever happened. A file that cannot be opened is reported
/// and skipped; a write failure or a limit of the format ends the job. The
/// games stored before a stop or a failure stay.
///
/// # Errors
///
/// [`BaseError::Cancelled`] when cancelled before anything was done; the
/// database errors (`Stale`, `ReadOnly`, `AlreadyExists`, `AlreadyOpen`...)
/// when it cannot be opened or created. Failures after that are in
/// [`FileImportOutcome::error`].
pub(crate) fn import_run(
    job: &FileImportJob,
    control: &OpenControl,
) -> Result<FileImportOutcome, BaseError> {
    run(job, control, &mut |_| {})
}

/// [`import_run`] with a hook called after each file is done (tests stop the
/// job at a known point with it).
#[allow(clippy::too_many_lines)] // one flat loop over the files
pub(crate) fn run(
    job: &FileImportJob,
    control: &OpenControl,
    after_file: &mut dyn FnMut(usize),
) -> Result<FileImportOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    // Declared before the writer, so on unwinding it drops after it
    // (Windows does not delete open files).
    let mut guard = CreatedGuard::default();
    let (mut writer, created) = open_target(&job.target)?;
    let path = job.target.path().to_owned();
    if created {
        guard.arm(&path);
    }
    let token = control.token();
    let mut progress = control.reporter();
    let total_bytes: u64 = job
        .files
        .iter()
        .map(|f| std::fs::metadata(f).map_or(0, |m| m.len()))
        .sum();
    let options = ImportOptions::default();
    let mut report = FileImportReport::default();
    let mut error = None;
    let mut done_bytes = 0u64;
    progress(0, total_bytes);
    for (index, file) in job.files.iter().enumerate() {
        if token.is_cancelled() {
            report.interrupted = true;
            break;
        }
        job.progress.set(index, u64::from(report.games_added));
        // A source that is one of the files of the database would be read
        // while it is written (D1996).
        if is_database_file(&path, file) {
            report.file_errors.push((index, BaseError::SourceIsTarget));
            continue;
        }
        let size = std::fs::metadata(file).map_or(0, |m| m.len());
        let base = done_bytes;
        let mut sink = |read: u64, _total: u64| progress(base.saturating_add(read), total_bytes);
        match import_pgn_file(file, &mut writer, &options, None, &mut sink, &token) {
            Ok(done) => {
                let stop = absorb(&mut report, index, &done);
                guard.added = report.games_added;
                report.files_read += 1;
                if done.interrupted {
                    report.interrupted = true;
                    break;
                }
                if let Some(stop) = stop {
                    error = Some(stop);
                    break;
                }
            }
            Err(ImportError::Open { path, source }) => {
                report
                    .file_errors
                    .push((index, BaseError::File(FileError::from_io(&source, path))));
            }
            Err(ImportError::Write {
                source,
                report: partial,
                ..
            }) => {
                absorb(&mut report, index, &partial);
                guard.added = report.games_added;
                report.files_read += 1;
                error = Some(BaseError::from_si4(&source));
                break;
            }
            Err(ImportError::ReadOnly) => {
                error = Some(BaseError::ReadOnly);
                break;
            }
            Err(_) => {
                error = Some(BaseError::Internal);
                break;
            }
        }
        done_bytes = done_bytes.saturating_add(size);
        after_file(index);
    }
    job.progress
        .set(job.files.len(), u64::from(report.games_added));
    progress(total_bytes, total_bytes);
    // A live writer blocks the reads of the index on Windows.
    drop(writer);
    // The guard has nothing left to do once this function decides.
    guard.disarm();
    // A stop before the first game was stored changed nothing (D1978).
    if report.interrupted && report.games_added == 0 && error.is_none() {
        if created {
            remove_created(&path);
        }
        return Err(BaseError::Cancelled);
    }
    if created && report.games_added == 0 {
        remove_created(&path);
        return Ok(FileImportOutcome {
            source: None,
            stamp: None,
            path,
            created,
            removed: true,
            report,
            error,
        });
    }
    let source = match Si4Database::open(&path) {
        Ok(db) => Some(SharedSource::new(BaseSource::Si4(Box::new(db)))),
        Err(e) => {
            tracing::error!(error = ?e, path = %path.display(), added = report.games_added, "the database cannot be opened again after the PGN import");
            error = error.or(Some(BaseError::from_si4(&e)));
            None
        }
    };
    Ok(FileImportOutcome {
        source,
        stamp: FileStamp::of(&path),
        path,
        created,
        removed: false,
        report,
        error,
    })
}

/// Removes the database this job created if the job ends without storing a
/// game and without deciding (a panic of the import, which `perform` turns
/// into an internal error): nobody is left holding an empty database
/// (M3-8 review, D1992).
#[derive(Default)]
struct CreatedGuard {
    path: Option<std::path::PathBuf>,
    added: u32,
}

impl CreatedGuard {
    fn arm(&mut self, path: &Path) {
        self.path = Some(path.to_owned());
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for CreatedGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.path
            && self.added == 0
        {
            remove_created(path);
        }
    }
}

/// Opens the database to write (checking that it is still the one the slot
/// read) or creates the new one.
fn open_target(target: &FileImportTarget) -> Result<(Si4Writer, bool), BaseError> {
    match target {
        FileImportTarget::Existing {
            path,
            expected_total,
            stamp,
        } => {
            if stamp.is_some_and(|expected| FileStamp::of(path) != Some(expected)) {
                return Err(BaseError::Stale);
            }
            let writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
            if writer.len() != *expected_total {
                return Err(BaseError::Stale);
            }
            Ok((writer, false))
        }
        FileImportTarget::New { path } => Ok((
            Si4Writer::create(path, NEW_BASE_TYPE).map_err(|e| BaseError::from_si4(&e))?,
            true,
        )),
    }
}

/// Whether `source` is one of the three files of the database at `base`.
fn is_database_file(base: &Path, source: &Path) -> bool {
    ["si4", "sn4", "sg4"]
        .iter()
        .any(|ext| crate::fsutil::same_file(source, &base.with_extension(ext)))
}

/// Deletes the three files of a database that this job created and that
/// holds no game (the creation refuses names that exist, so they are ours).
fn remove_created(path: &Path) {
    for extension in ["si4", "sn4", "sg4"] {
        let file = path.with_extension(extension);
        if let Err(e) = std::fs::remove_file(&file)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error = %e, path = %file.display(), "cannot remove the empty database");
        }
    }
}

/// Adds the report of one file to the totals; returns the error that ends
/// the job (a limit of the format), if the file hit one.
fn absorb(total: &mut FileImportReport, file: usize, done: &DbReport) -> Option<BaseError> {
    total.games_read = total.games_read.saturating_add(done.games_read);
    total.games_added = total.games_added.saturating_add(done.games_added);
    total.games_skipped = total.games_skipped.saturating_add(done.games_skipped_total);
    let push = |total: &mut FileImportReport, game: u64, line: u64, severity, key| {
        if severity >= Severity::Warning {
            total.warnings = total.warnings.saturating_add(1);
        }
        if total.lines.len() < MAX_REPORT_LINES {
            total.lines.push(ImportLine {
                file,
                game,
                line,
                severity,
                key,
            });
        } else {
            total.lines_dropped = total.lines_dropped.saturating_add(1);
        }
    };
    for d in &done.diagnostics {
        // Informative remarks (text between games...) are not logged by the
        // Scid either.
        if d.severity >= Severity::Warning {
            push(total, d.game, d.line, d.severity, pgn_message_key(&d.kind));
        }
    }
    for n in &done.notices {
        push(
            total,
            n.game,
            n.line,
            Severity::Warning,
            notice_key(&n.kind),
        );
    }
    for (game, reason) in &done.games_skipped {
        push(
            total,
            u64::from(*game),
            0,
            Severity::Error,
            skip_key(reason),
        );
    }
    total.warnings = total
        .warnings
        .saturating_add(done.messages_dropped.min(u64::MAX / 2));
    total.lines_dropped = total.lines_dropped.saturating_add(done.messages_dropped);
    match &done.stopped {
        Some(ImportStop::GameLimit { limit }) => Some(BaseError::TooManyGames { limit: *limit }),
        Some(ImportStop::NameLimit { limit, .. }) => Some(BaseError::NameLimit { limit: *limit }),
        Some(stop) => {
            tracing::warn!(file, reason = %stop, "a PGN file could not be read to the end");
            push(total, 0, 0, Severity::Error, "file-import-stop-read");
            None
        }
        None => None,
    }
}

fn notice_key(kind: &ImportNoticeKind) -> &'static str {
    match kind {
        ImportNoticeKind::TagValueTruncated { .. } => "file-import-notice-tag-truncated",
        ImportNoticeKind::TooManyExtraTags { .. } => "file-import-notice-extra-tags",
        ImportNoticeKind::EmptyTagName => "file-import-notice-empty-tag",
        ImportNoticeKind::TagNameTooLong => "file-import-notice-tag-name",
        ImportNoticeKind::NameTruncated => "file-import-notice-name-truncated",
        ImportNoticeKind::UnknownFlagChar(_) => "file-import-notice-flag",
        ImportNoticeKind::NoGamesFound => "file-import-notice-no-games",
        ImportNoticeKind::TagDropped { .. } => "file-import-notice-tag-dropped",
        _ => "file-import-notice-other",
    }
}

fn skip_key(reason: &SkipReason) -> &'static str {
    match reason {
        SkipReason::RecordTooLarge { .. } => "file-import-skip-too-large",
        SkipReason::UnsupportedVariant => "file-import-skip-variant",
        SkipReason::GameTooLarge => "file-import-skip-game-too-large",
        SkipReason::Unencodable(_) => "file-import-skip-unencodable",
        _ => "file-import-skip-other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch_base(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("g.si4");
        for ext in ["si4", "sn4", "sg4"] {
            std::fs::write(path.with_extension(ext), b"x").unwrap();
        }
        path
    }

    fn left(path: &Path) -> usize {
        ["si4", "sn4", "sg4"]
            .iter()
            .filter(|e| path.with_extension(e).exists())
            .count()
    }

    #[test]
    fn the_guard_removes_an_empty_new_database_when_the_job_panics() {
        let dir = tempfile::tempdir().unwrap();
        let path = touch_base(dir.path());
        let result = std::panic::catch_unwind(|| {
            let mut guard = CreatedGuard::default();
            guard.arm(&path);
            panic!("the import panicked");
        });
        assert!(result.is_err());
        assert_eq!(left(&path), 0);
    }

    #[test]
    fn the_guard_keeps_a_database_with_games_and_one_that_was_decided() {
        let dir = tempfile::tempdir().unwrap();
        let path = touch_base(dir.path());
        {
            let mut guard = CreatedGuard::default();
            guard.arm(&path);
            guard.added = 3;
        }
        assert_eq!(left(&path), 3);
        {
            let mut guard = CreatedGuard::default();
            guard.arm(&path);
            guard.disarm();
        }
        assert_eq!(left(&path), 3);
        drop(CreatedGuard::default());
        assert_eq!(left(&path), 3, "an unarmed guard never deletes");
    }
}
