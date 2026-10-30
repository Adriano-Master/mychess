//! The blocking work behind the effects of the ECO tools. `perform` runs it
//! on `spawn_blocking`; `update` never calls it.

use mychess_db::si4::{Si4Database, Si4Writer};
use mychess_db::{
    ClassifyError, ClassifyReport, ClassifySelection, EcoScope, EcoStatsTable, Filter,
    NoveltyError, NoveltyOptions, classify_database, classify_preview, find_novelty,
};

use super::message::{
    ClassifyOutcome, ClassifyScope, EcoClassifyJob, EcoNoveltyJob, NoveltyOutcome, SharedStats,
};
use crate::bases::{BaseError, BaseKind, BaseSource, FileStamp, OpenControl, SharedSource};
use mychess_db::GameSource;

/// Counts the games of `source` by ECO code (only the games of `filter`
/// when it is given).
pub(crate) fn stats_run(source: &SharedSource, filter: Option<&Filter>) -> SharedStats {
    let guard = source.read();
    let scope = filter.map_or(EcoScope::WholeBase, EcoScope::Filter);
    SharedStats::new(EcoStatsTable::build(&*guard, scope))
}

fn selection(scope: &ClassifyScope) -> ClassifySelection<'_> {
    match scope {
        ClassifyScope::All => ClassifySelection::All,
        ClassifyScope::NoCode => ClassifySelection::NoCode,
        ClassifyScope::Filter(filter) => ClassifySelection::Filter(filter),
        ClassifyScope::Since(date) => ClassifySelection::Since(*date),
    }
}

/// How a run that cannot write or count a database ends.
fn failed(error: BaseError, report: ClassifyReport, write: bool) -> ClassifyOutcome {
    ClassifyOutcome {
        source: None,
        stamp: None,
        report,
        error: Some(error),
        write,
    }
}

/// Classifies the games of the job's database: writes the codes in the
/// `.si4` file and opens it again (`write`), or only counts the games that
/// would change.
///
/// Like the flag change, the write first checks that the file is still the
/// one the slot read (its size and time and the number of games), and the
/// short-lived [`Si4Writer`] is dropped before the file is read again (on
/// Windows a live writer blocks reads of the index). What was written before
/// a failure or a cancel stays; the database is opened again whatever
/// happened, so the slot shows what is on disk.
///
/// # Errors
///
/// [`BaseError::Cancelled`] before anything is done; the other failures are
/// in [`ClassifyOutcome::error`] with the partial counters.
pub(crate) fn classify_run(
    job: &EcoClassifyJob,
    control: &OpenControl,
) -> Result<ClassifyOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let token = control.token();
    let mut progress = control.reporter();
    let select = selection(&job.scope);
    if !job.write {
        // Not inside the `match`: its temporaries would keep the read lock
        // while the other arm takes the write lock.
        let kind = job.source.read().kind();
        let report = if let (Some(path), BaseKind::Si4) = (&job.path, kind) {
            // An SI4 file is read through a reader of its own, so the
            // database stays free for the list while the games are counted.
            let mut db = Si4Database::open(path).map_err(|e| BaseError::from_si4(&e))?;
            classify_preview(
                &mut db,
                &job.book,
                select,
                job.extended,
                &token,
                &mut progress,
            )
        } else {
            let mut guard = job.source.lock();
            classify_preview(
                &mut *guard,
                &job.book,
                select,
                job.extended,
                &token,
                &mut progress,
            )
        };
        return Ok(match report {
            Ok(report) => ClassifyOutcome {
                source: None,
                stamp: None,
                report,
                error: report.cancelled.then_some(BaseError::Cancelled),
                write: false,
            },
            Err(error) => {
                let (mapped, partial) = map_error(&error);
                failed(mapped, partial, false)
            }
        });
    }
    let Some(path) = job.path.as_deref() else {
        return Err(BaseError::ReadOnly);
    };
    let mut report = ClassifyReport::default();
    let mut error = None;
    if job
        .stamp
        .is_some_and(|expected| FileStamp::of(path) != Some(expected))
    {
        error = Some(BaseError::Stale);
    } else {
        let mut writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
        if writer.len() == job.expected_total {
            match classify_database(
                &mut writer,
                &job.book,
                select,
                job.extended,
                &token,
                &mut progress,
            ) {
                Ok(done) => {
                    report = done;
                    if done.cancelled {
                        error = Some(BaseError::Cancelled);
                    }
                }
                Err(e) => {
                    let (mapped, partial) = map_error(&e);
                    report = partial;
                    error = Some(mapped);
                }
            }
        } else {
            error = Some(BaseError::Stale);
        }
        drop(writer);
    }
    let source = match Si4Database::open(path) {
        Ok(db) => Some(SharedSource::new(BaseSource::Si4(Box::new(db)))),
        Err(e) => {
            tracing::error!(error = ?e, path = %path.display(), written = report.written, "the database cannot be opened again after the ECO classification");
            error = error.or(Some(BaseError::from_si4(&e)));
            None
        }
    };
    Ok(ClassifyOutcome {
        source,
        stamp: FileStamp::of(path),
        report,
        error,
        write: true,
    })
}

/// The error of the engine as the application's, with the partial counters.
fn map_error(error: &ClassifyError) -> (BaseError, ClassifyReport) {
    match error {
        ClassifyError::Write { source, report } => (BaseError::from_si4(source), *report),
        _ => (BaseError::Internal, ClassifyReport::default()),
    }
}

/// Searches the novelty of the job's game in its reference database.
///
/// # Errors
///
/// [`BaseError`] when the `.si4` file of the reference cannot be opened.
pub(crate) fn novelty_run(
    job: &EcoNoveltyJob,
    control: &OpenControl,
) -> Result<NoveltyOutcome, BaseError> {
    let token = control.token();
    let mut progress = control.reporter();
    let options = NoveltyOptions {
        older_only: job.older_only,
        current_game: job.current_game,
        cancel: &token,
        progress: &mut progress,
    };
    let book = job.book.as_deref();
    let kind = job.source.read().kind();
    let result = if let (Some(path), BaseKind::Si4) = (&job.path, kind) {
        let mut db = Si4Database::open(path).map_err(|e| BaseError::from_si4(&e))?;
        find_novelty(&mut db, &job.game, book, options)
    } else {
        let mut guard = job.source.lock();
        let source: &mut dyn GameSource = &mut *guard;
        find_novelty(source, &job.game, book, options)
    };
    Ok(match result {
        Ok(found) => NoveltyOutcome::Found {
            ply: found.ply,
            san: found.san,
            move_number: found.move_number,
            color: found.color,
        },
        Err(NoveltyError::NoNovelty) => NoveltyOutcome::NoNovelty,
        Err(NoveltyError::NonStandardStart) => NoveltyOutcome::NonStandardStart,
        Err(NoveltyError::Interrupted) => NoveltyOutcome::Interrupted,
        Err(NoveltyError::Read { game, .. }) => NoveltyOutcome::Unreadable { game },
        Err(_) => NoveltyOutcome::Unsupported,
    })
}
