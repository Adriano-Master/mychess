//! The blocking work behind the effects of the spelling file and the name
//! checker. [`perform`] runs it on `spawn_blocking`; `update` never calls it.

use std::path::{Path, PathBuf};

use mychess_db::PlayerStats;
use mychess_db::si4::{Si4Database, Si4Writer};
use mychess_db::spelling::check::{ScanOptions, scan_names};
use mychess_db::spelling::elo::{AddEloError, EloOptions, EloReport, apply_add_elo, plan_add_elo};
use mychess_db::spelling::photo::PhotoLibrary;
use mychess_db::spelling::{ReadOptions, SpellingSet};

use super::message::{
    EloJob, EloOutcome, LoadedSpelling, NamesJob, NamesOutcome, PhotoCounts, PlanJob, PlannedElo,
    ScanDone, ScanJob, Shared, SharedSpelling, SpellRow, SpellingCounts, SpellingEffect,
    SpellingLoadError, SpellingMessage, SpellingNotice, WriteOutcome, db_kind,
};
use crate::Message;
use crate::bases::{BaseError, BaseSource, FileStamp, OpenControl, SharedSource};
use crate::tree::LockingSource;

/// The data directory of the program (`None` when the system has no home).
pub(crate) fn data_dir(over: Option<&Path>) -> Option<PathBuf> {
    match over {
        Some(dir) => Some(dir.to_path_buf()),
        None => crate::start::data_dir(),
    }
}

/// The file the start-up loads: `spelling.ssp`, else `spelling.ssp.gz` of
/// the data directory (D3335). The first name when neither exists, so the
/// error names a file.
fn default_file(dir: &Path) -> PathBuf {
    let plain = dir.join("spelling.ssp");
    let packed = dir.join("spelling.ssp.gz");
    if !plain.is_file() && packed.is_file() {
        packed
    } else {
        plain
    }
}

/// Reads the spelling file (the work of [`SpellingEffect::Load`]).
pub(crate) fn load_run(
    path: Option<PathBuf>,
    over: Option<&Path>,
    control: &OpenControl,
) -> Result<LoadedSpelling, SpellingLoadError> {
    let path = if let Some(path) = path {
        path
    } else {
        let dir = data_dir(over).ok_or(SpellingLoadError::File(
            crate::error::FileErrorKind::NotFound,
        ))?;
        default_file(&dir)
    };
    let token = control.token();
    let mut progress = control.reporter();
    let set = SpellingSet::read(&path, &ReadOptions::default(), &token, &mut progress)
        .map_err(|e| SpellingLoadError::from_db(&e))?;
    if control.is_cancelled() {
        return Err(SpellingLoadError::Cancelled);
    }
    let report = *set.report();
    let counts = SpellingCounts {
        players: report.players,
        events: report.events,
        sites: report.sites,
        rounds: report.rounds,
        skipped_lines: report.skipped_lines,
        has_elo: set
            .checker(mychess_db::si4::NameKind::Player)
            .has_elo_data(),
    };
    Ok(LoadedSpelling {
        set: Shared::new(set),
        counts,
        path,
    })
}

/// Looks for photos in the data directory's `photos` and the extra folder.
pub(crate) fn photos_run(
    extra: Option<PathBuf>,
    over: Option<&Path>,
    control: &OpenControl,
) -> (PhotoLibrary, PhotoCounts) {
    let token = control.token();
    let mut library = PhotoLibrary::new();
    let mut errors = 0u32;
    let mut dirs = Vec::new();
    if let Some(dir) = data_dir(over) {
        dirs.push(dir.join("photos"));
    }
    dirs.extend(extra);
    for dir in dirs {
        errors = errors.saturating_add(library.load_dir(&dir, &token).errors);
    }
    let counts = PhotoCounts {
        photos: u32::try_from(library.len()).unwrap_or(u32::MAX),
        errors,
    };
    (library, counts)
}

/// The highest Elo of every player, raised to the peak ratings of the
/// spelling file (`recalcEstimatedRatings`, rule 87).
pub(crate) fn stats_run(
    source: SharedSource,
    set: &SharedSpelling,
    control: &OpenControl,
) -> Option<PlayerStats> {
    if control.is_cancelled() {
        return None;
    }
    let db = LockingSource::new(source);
    let mut stats = PlayerStats::from_database_cancellable(&db, &control.token())?;
    if control.is_cancelled() {
        return None;
    }
    stats.apply_spelling_estimates(&db, set.checker(mychess_db::si4::NameKind::Player));
    (!control.is_cancelled()).then_some(stats)
}

/// Looks for corrections of one kind of name (rules 37 to 47).
pub(crate) fn scan_run(job: &ScanJob, control: &OpenControl) -> ScanDone {
    let db = LockingSource::new(job.source.clone());
    let kind = db_kind(job.kind);
    let mut options = ScanOptions::new(kind);
    options.surnames = job.surnames;
    options.ambiguous = job.ambiguous;
    let mut progress = control.reporter();
    let result = scan_names(
        &db,
        job.set.checker(kind),
        options,
        &control.token(),
        &mut progress,
    );
    ScanDone {
        interrupted: result.interrupted,
        rows: result
            .proposals
            .into_iter()
            .map(|p| SpellRow {
                selected: !p.ambiguous,
                old: p.old,
                new: p.new,
                games: p.games,
                ambiguous: p.ambiguous,
                born: p.born,
                died: p.died,
            })
            .collect(),
    }
}

/// The corrections of the rows to apply, as the writer wants them.
pub(crate) fn changes_of(rows: &[SpellRow]) -> Vec<mychess_db::si4::NameChange> {
    rows.iter()
        .filter(|r| r.selected && !r.new.trim().is_empty() && r.new.trim() != r.old)
        .map(|r| mychess_db::si4::NameChange {
            old: r.old.clone(),
            new: r.new.trim().to_owned(),
            born: r.born,
            died: r.died,
        })
        .collect()
}

/// The text of the Scid for the rows (the "copy" button).
pub(crate) fn rows_text(kind: crate::game_info::NameKind, rows: &[SpellRow]) -> String {
    let proposals: Vec<mychess_db::spelling::check::Proposal> = rows
        .iter()
        .map(|r| mychess_db::spelling::check::Proposal {
            old: r.old.clone(),
            new: r.new.clone(),
            games: r.games,
            ambiguous: r.ambiguous,
            born: r.born,
            died: r.died,
        })
        .collect();
    mychess_db::spelling::apply::proposals_to_text(db_kind(kind), &proposals)
}

/// Works out "Add Elo" without writing (rules 65 to 69).
pub(crate) fn plan_run(job: &PlanJob, control: &OpenControl) -> Result<PlannedElo, SpellingNotice> {
    let db = LockingSource::new(job.source.clone());
    let options = EloOptions {
        overwrite: job.overwrite,
        only_filter: job.only_filter,
        no_month: true,
        update: true,
    };
    let mut progress = control.reporter();
    let plan = plan_add_elo(
        &db,
        job.set.checker(mychess_db::si4::NameKind::Player),
        job.filter.as_deref(),
        options,
        &control.token(),
        &mut progress,
    )
    .map_err(|error| match error {
        AddEloError::NoEloData => SpellingNotice::NoEloData,
        AddEloError::FilterSize { .. } => SpellingNotice::Stale,
        _ => SpellingNotice::NotLoaded,
    })?;
    Ok(PlannedElo {
        report: plan.report,
        plan: Shared::new(plan),
    })
}

/// Reads the `.si4` file again into a shared source.
fn reopen(path: &Path) -> Result<SharedSource, BaseError> {
    Si4Database::open(path)
        .map(|db| SharedSource::new(BaseSource::Si4(Box::new(db))))
        .map_err(|e| BaseError::from_si4(&e))
}

/// Writes the corrected names (rules 48 to 59): in the `.si4` files through
/// a short-lived writer, as `classify_run` does, or in memory for the
/// Clipbase. The slot's database is read again whatever happened.
///
/// # Errors
///
/// [`BaseError`] when nothing could be tried; the failures after the writer
/// opened are in [`WriteOutcome::error`] with the partial counts.
pub(crate) fn names_run(job: &NamesJob, control: &OpenControl) -> Result<NamesOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let token = control.token();
    let mut progress = control.reporter();
    let kind = db_kind(job.kind);
    let Some(path) = job.path.as_deref() else {
        let mut guard = job.source.lock();
        let clip = guard.clipbase_mut().ok_or(BaseError::Internal)?;
        let report = clip.apply_name_changes(kind, &job.changes, &token, &mut progress);
        return Ok(WriteOutcome {
            error: report.interrupted.then_some(BaseError::Cancelled),
            report,
            source: None,
            stamp: None,
            in_memory: true,
        });
    };
    let mut report = mychess_db::si4::NameChangeReport::default();
    let mut error = None;
    if job
        .stamp
        .is_some_and(|expected| FileStamp::of(path) != Some(expected))
    {
        error = Some(BaseError::Stale);
    } else {
        let mut writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
        if writer.len() == job.expected_total {
            match writer.apply_name_changes(kind, &job.changes, &token, &mut progress) {
                Ok(done) => {
                    report = done;
                    if done.interrupted {
                        error = Some(BaseError::Cancelled);
                    }
                }
                Err(e) => error = Some(BaseError::from_si4(&e)),
            }
        } else {
            error = Some(BaseError::Stale);
        }
        // On Windows a live writer blocks the reading of the index.
        drop(writer);
    }
    let source = match reopen(path) {
        Ok(source) => Some(source),
        Err(e) => {
            tracing::error!(error = ?e, path = %path.display(), "the database cannot be opened again after the name corrections");
            error = error.or(Some(e));
            None
        }
    };
    Ok(WriteOutcome {
        report,
        source,
        stamp: FileStamp::of(path),
        error,
        in_memory: false,
    })
}

/// Writes the ratings of an "Add Elo" plan (rules 65 to 72).
///
/// # Errors
///
/// As [`names_run`].
pub(crate) fn elo_run(job: &EloJob, control: &OpenControl) -> Result<EloOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let token = control.token();
    let mut progress = control.reporter();
    let Some(path) = job.path.as_deref() else {
        let mut guard = job.source.lock();
        let clip = guard.clipbase_mut().ok_or(BaseError::Internal)?;
        let games = clip.apply_elo_changes(&job.plan.changes);
        return Ok(WriteOutcome {
            report: EloReport {
                ratings: job.plan.report.ratings,
                games,
                interrupted: false,
            },
            source: None,
            stamp: None,
            error: None,
            in_memory: true,
        });
    };
    let mut report = EloReport::default();
    let mut error = None;
    if job
        .stamp
        .is_some_and(|expected| FileStamp::of(path) != Some(expected))
    {
        error = Some(BaseError::Stale);
    } else {
        let mut writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
        if writer.len() == job.expected_total {
            let (done, result) = apply_add_elo(&mut writer, &job.plan, &token, &mut progress);
            report = done;
            match result {
                Ok(()) if done.interrupted => error = Some(BaseError::Cancelled),
                Ok(()) => {}
                Err(e) => error = Some(BaseError::from_si4(&e)),
            }
        } else {
            error = Some(BaseError::Stale);
        }
        drop(writer);
    }
    let source = match reopen(path) {
        Ok(source) => Some(source),
        Err(e) => {
            tracing::error!(error = ?e, path = %path.display(), "the database cannot be opened again after Add Elo");
            error = error.or(Some(e));
            None
        }
    };
    Ok(WriteOutcome {
        report,
        source,
        stamp: FileStamp::of(path),
        error,
        in_memory: false,
    })
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    on_panic: impl FnOnce() -> T,
) -> T {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| on_panic())
}

/// Runs an effect of the spelling file. Never panics: a task that dies is a
/// failed answer.
#[allow(clippy::too_many_lines)] // one flat match over the effects reads best
pub(crate) async fn perform(effect: SpellingEffect) -> Message {
    let message = match effect {
        SpellingEffect::Load {
            request,
            path,
            data_dir,
            quiet: _,
            control,
        } => {
            let result = blocking(
                move || load_run(path, data_dir.as_deref(), &control),
                || Err(SpellingLoadError::Internal),
            )
            .await;
            SpellingMessage::Loaded {
                request,
                result: Box::new(result),
            }
        }
        SpellingEffect::Photos {
            request,
            extra,
            data_dir,
            control,
        } => {
            let (library, counts) = blocking(
                move || photos_run(extra, data_dir.as_deref(), &control),
                || (PhotoLibrary::new(), PhotoCounts::default()),
            )
            .await;
            SpellingMessage::PhotosLoaded {
                request,
                library: Shared::new(library),
                counts,
            }
        }
        SpellingEffect::Stats {
            request,
            source,
            set,
            control,
        } => {
            let stats = blocking(move || stats_run(source, &set, &control), || None).await;
            SpellingMessage::StatsBuilt {
                request,
                stats: stats.map(Shared::new),
            }
        }
        SpellingEffect::Scan {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || scan_run(&job, &control),
                || ScanDone {
                    rows: Vec::new(),
                    interrupted: true,
                },
            )
            .await;
            SpellingMessage::Scanned {
                request,
                result: Box::new(result),
            }
        }
        SpellingEffect::PlanElo {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || plan_run(&job, &control),
                || Err(SpellingNotice::Failed(BaseError::Internal)),
            )
            .await;
            SpellingMessage::Planned {
                request,
                result: Box::new(result),
            }
        }
        SpellingEffect::ApplyNames {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || names_run(&job, &control),
                || Err(BaseError::Internal),
            )
            .await;
            if let Ok(done) = &result {
                tracing::debug!(
                    mapped = done.report.mapped,
                    changed = done.report.games_changed,
                    error = ?done.error,
                    "name corrections ended"
                );
            }
            SpellingMessage::Applied {
                request,
                result: Box::new(result),
            }
        }
        SpellingEffect::ApplyElo {
            request,
            job,
            control,
        } => {
            let result =
                blocking(move || elo_run(&job, &control), || Err(BaseError::Internal)).await;
            SpellingMessage::EloApplied {
                request,
                result: Box::new(result),
            }
        }
    };
    Message::Spelling(message)
}

#[cfg(test)]
mod tests {
    use mychess_core::PartialDate;

    use super::*;

    #[test]
    fn a_missing_default_file_names_the_plain_one() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(default_file(dir.path()), dir.path().join("spelling.ssp"));
        std::fs::write(dir.path().join("spelling.ssp.gz"), b"").expect("write");
        assert_eq!(default_file(dir.path()), dir.path().join("spelling.ssp.gz"));
        std::fs::write(dir.path().join("spelling.ssp"), b"").expect("write");
        assert_eq!(default_file(dir.path()), dir.path().join("spelling.ssp"));
    }

    #[test]
    fn rows_to_changes_keep_only_ticked_edited_rows() {
        let row = |old: &str, new: &str, selected: bool| SpellRow {
            old: old.to_owned(),
            new: new.to_owned(),
            games: 1,
            ambiguous: false,
            born: PartialDate::UNKNOWN,
            died: PartialDate::UNKNOWN,
            selected,
        };
        let rows = [
            row("a", "b", true),
            row("c", "d", false),
            row("e", "e", true),
            row("f", "  ", true),
        ];
        let changes = changes_of(&rows);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].old, "a");
    }
}
