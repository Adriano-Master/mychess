//! The blocking work behind the effects of the maintenance tools. [`perform`]
//! runs it on `spawn_blocking`; `update` never calls it.
//!
//! The jobs that write follow `classify_run` of the ECO tools: the
//! `FileStamp` is checked first, the reader of the slot is let go (on Windows
//! a live writer locks the index and a rename needs the files free), a
//! short-lived [`Si4Writer`] does the work and is dropped, and the database is
//! opened again whatever happened. What was written before a failure or a
//! stop stays (the operations of the database crate leave a database that
//! opens).

use std::path::{Path, PathBuf};

use mychess_core::Game;
use mychess_db::crosstable::{TournamentReference, select_tournament};
use mychess_db::maintenance::{
    MaintError, Scope, check_games, compaction_stats, database_stats, diff_games, filter_by_tag,
    find_duplicates, list_extra_tags, name_usage, repair_database, share_tags_preview,
};
use mychess_db::si4::{
    FailPoint, FlagOp, GameFlags, NameEditError, NameScope, RepairOptions, Si4Database, Si4Error,
    Si4Writer, compact_games, compact_names, sort_games_controlled,
};
use mychess_db::{Filter, GameSource};

use super::cleaner;
use super::message::{
    CompactPreview, HeaderEdit, MaintJob, MaintOp, MaintOutcome, MaintReport, MaintenanceEffect,
    MaintenanceMessage, NameJobGames, PreviewDone, PreviewResult, PreviewWork,
};
use crate::Message;
use crate::bases::{
    BaseError, BaseKind, BaseSource, Clipbase, FileStamp, OpenControl, SharedSource,
};
use crate::fsutil::same_file;
use crate::spelling_ui::Shared;
use crate::tree::LockingSource;

/// The result of the body of a job: the report, whether something was
/// written, and why it stopped early.
struct Body {
    report: MaintReport,
    changed: bool,
    error: Option<BaseError>,
}

impl Body {
    fn failed(error: BaseError) -> Body {
        Body {
            report: MaintReport::Nothing,
            changed: false,
            error: Some(error),
        }
    }
}

/// The error of the database as the application's; an invalid tag is told
/// apart (`Unencodable`) so the notice can name the value.
fn base_error(error: &Si4Error) -> BaseError {
    match error {
        Si4Error::InvalidTag { .. } => BaseError::Unencodable,
        other => BaseError::from_si4(other),
    }
}

/// Reads the `.si4` file again into a shared source.
fn reopen(path: &Path) -> Result<SharedSource, BaseError> {
    Si4Database::open(path)
        .map(|db| SharedSource::new(BaseSource::Si4(Box::new(db))))
        .map_err(|e| BaseError::from_si4(&e))
}

/// Lets go of the reader of the slot (its files and its handles) by putting
/// an empty database in its place. The slot is given a new source when the
/// job ends; nothing else reads this one meanwhile (the state keeps the
/// readers off a slot that is being rewritten).
fn release_reader(source: &SharedSource) -> Result<(), BaseError> {
    let placeholder = Clipbase::new(0)?;
    let mut guard = source.lock();
    if matches!(&*guard, BaseSource::Si4(_)) {
        *guard = BaseSource::Clipbase(Box::new(placeholder));
    }
    Ok(())
}

/// Opens the writer of the file if it still holds the games the slot read.
fn open_checked(path: &Path, expected: u32) -> Result<Si4Writer, BaseError> {
    let writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
    if writer.len() == expected {
        Ok(writer)
    } else {
        Err(BaseError::Stale)
    }
}

/// Copies the three files of the database next to the originals, without
/// overwriting an older copy: `name.si4.bak`, then `.bak1`, `.bak2`...
///
/// # Errors
///
/// [`BaseError::File`] when a copy fails; the partial set is removed again.
pub(crate) fn backup_files(path: &Path, control: &OpenControl) -> Result<Vec<PathBuf>, BaseError> {
    let exts = ["si4", "sn4", "sg4"];
    let target = |ext: &str, generation: u32| {
        let suffix = if generation == 0 {
            "bak".to_owned()
        } else {
            format!("bak{generation}")
        };
        path.with_extension(format!("{ext}.{suffix}"))
    };
    let generation = (0u32..10_000)
        .find(|g| exts.iter().all(|e| !target(e, *g).exists()))
        .ok_or(BaseError::AlreadyExists)?;
    let mut made: Vec<PathBuf> = Vec::new();
    for ext in exts {
        if control.is_cancelled() {
            remove_all(&made);
            return Err(BaseError::Cancelled);
        }
        let from = path.with_extension(ext);
        let to = target(ext, generation);
        let temp = to.with_extension("part");
        let copied = std::fs::copy(&from, &temp).and_then(|_| std::fs::rename(&temp, &to));
        if let Err(error) = copied {
            // Best effort: a copy that is left over is harmless.
            drop(std::fs::remove_file(&temp));
            remove_all(&made);
            return Err(BaseError::File(crate::error::FileError::from_io(
                &error, to,
            )));
        }
        made.push(to);
    }
    Ok(made)
}

fn remove_all(files: &[PathBuf]) {
    for file in files {
        // Best effort: the copies of a failed backup are not needed.
        drop(std::fs::remove_file(file));
    }
}

/// Runs `f` on the games of the job's database for reading. An `.si4` is
/// read through a reader of its own (the list stays free); the other
/// databases are read a game at a time under the lock of the slot.
fn with_reader<T>(
    job: &MaintJob,
    f: impl FnOnce(&mut dyn GameSource) -> T,
) -> Result<T, BaseError> {
    let kind = job.source.read().kind();
    if let (Some(path), BaseKind::Si4) = (&job.path, kind) {
        let mut db = Si4Database::open(path).map_err(|e| BaseError::from_si4(&e))?;
        Ok(f(&mut db))
    } else {
        let mut db = LockingSource::new(job.source.clone());
        Ok(f(&mut db))
    }
}

fn scope_of(filter: Option<&Filter>) -> Scope<'_> {
    filter.map_or(Scope::All, Scope::Filter)
}

fn maint_error(error: &MaintError) -> BaseError {
    match error {
        MaintError::FilterSize { .. } => BaseError::Stale,
        MaintError::Si4(e) => base_error(e),
        MaintError::Repair(e) => base_error(&e.source),
        MaintError::Write(_) => BaseError::Internal,
    }
}

/// The jobs that only read.
fn read_job(job: &MaintJob, control: &OpenControl) -> Result<MaintOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let token = control.token();
    let mut progress = control.reporter();
    let body = with_reader(job, |db| match &job.op {
        MaintOp::Check { filter } => {
            match check_games(db, scope_of(filter.as_deref()), &mut progress, &token) {
                Ok(report) => Body {
                    changed: false,
                    error: report.interrupted.then_some(BaseError::Cancelled),
                    report: MaintReport::Check(Shared::new(report)),
                },
                Err(error) => Body::failed(maint_error(&error)),
            }
        }
        MaintOp::Twins { criteria, filter } => {
            match find_duplicates(
                db,
                scope_of(filter.as_deref()),
                criteria,
                &mut progress,
                &token,
            ) {
                Ok(scan) => Body {
                    changed: false,
                    error: scan.report.interrupted.then_some(BaseError::Cancelled),
                    report: MaintReport::Twins(Shared::new(scan)),
                },
                Err(error) => Body::failed(maint_error(&error)),
            }
        }
        MaintOp::TagList { filter } => {
            match list_extra_tags(db, scope_of(filter.as_deref()), &mut progress, &token) {
                Ok(listing) => Body {
                    changed: false,
                    error: listing.interrupted.then_some(BaseError::Cancelled),
                    report: MaintReport::TagList(Shared::new(listing)),
                },
                Err(error) => Body::failed(maint_error(&error)),
            }
        }
        MaintOp::TagFilter { tag, filter } => {
            match filter_by_tag(db, scope_of(filter.as_deref()), tag, &mut progress, &token) {
                Ok(found) => Body {
                    changed: false,
                    error: found.interrupted.then_some(BaseError::Cancelled),
                    report: MaintReport::TagFilter {
                        matched: found.matched,
                        filter: Shared::new(found.filter),
                    },
                },
                Err(error) => Body::failed(maint_error(&error)),
            }
        }
        MaintOp::DropDeleted { filter } => drop_deleted(db, filter, &token, &mut progress),
        _ => Body::failed(BaseError::Internal),
    })?;
    let mut out = MaintOutcome::nothing();
    out.report = body.report;
    out.error = body.error;
    Ok(out)
}

/// The filter without the games that have the `D` flag (index only).
fn drop_deleted(
    db: &mut dyn GameSource,
    filter: &Filter,
    token: &mychess_db::CancelToken,
    progress: &mut dyn mychess_db::ProgressSink,
) -> Body {
    let total = u64::from(filter.count());
    let mut next = filter.clone();
    let mut removed = 0u32;
    for (done, number) in (0u64..).zip(filter.iter_games()) {
        if done.is_multiple_of(4096) {
            progress.progress(done, total);
            if token.is_cancelled() {
                return Body::failed(BaseError::Cancelled);
            }
        }
        if db
            .entry(number)
            .is_ok_and(|e| e.flags.contains(GameFlags::DELETE))
        {
            next.set(number, 0);
            removed += 1;
        }
    }
    progress.progress(total, total);
    Body {
        report: MaintReport::FilterWithoutDeleted {
            filter: Shared::new(next),
            removed,
        },
        changed: false,
        error: None,
    }
}

/// The jobs that write the files of an `.si4` database.
fn file_write(
    job: &MaintJob,
    control: &OpenControl,
    body: impl FnOnce(&Path) -> Body,
) -> Result<MaintOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let path = job.path.as_deref().ok_or(BaseError::ReadOnly)?;
    release_reader(&job.source)?;
    let mut out = MaintOutcome::nothing();
    if job
        .stamp
        .is_some_and(|expected| FileStamp::of(path) != Some(expected))
    {
        out.error = Some(BaseError::Stale);
    } else {
        if job.backup {
            match backup_files(path, control) {
                Ok(files) => out.backups = files,
                Err(error) => out.error = Some(error),
            }
        }
        if out.error.is_none() {
            let done = body(path);
            out.report = done.report;
            out.changed = done.changed;
            out.error = done.error;
        }
    }
    match reopen(path) {
        Ok(source) => out.source = Some(source),
        Err(error) => {
            tracing::error!(?error, path = %path.display(), "the database cannot be opened again after the maintenance");
            out.error = out.error.or(Some(error));
        }
    }
    out.stamp = FileStamp::of(path);
    Ok(out)
}

/// The jobs that change the Clipbase in place.
fn clip_write(
    job: &MaintJob,
    control: &OpenControl,
    body: impl FnOnce(&mut Clipbase) -> Body,
) -> Result<MaintOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let mut guard = job.source.lock();
    let clip = guard.clipbase_mut().ok_or(BaseError::Internal)?;
    if GameSource::len(&*clip) != job.expected_total {
        return Err(BaseError::Stale);
    }
    let done = body(clip);
    let mut out = MaintOutcome::nothing();
    out.report = done.report;
    out.changed = done.changed;
    out.error = done.error;
    out.in_memory = true;
    Ok(out)
}

fn flags_body(
    writer: &mut Si4Writer,
    games: &[u32],
    flag: GameFlags,
    action: FlagOp,
) -> (u32, Option<BaseError>) {
    let (changed, result) = writer.set_flags_batch_counted(games, flag, action);
    (changed, result.err().map(|e| base_error(&e)))
}

fn header_body(path: &Path, expected: u32, edit: &HeaderEdit) -> Body {
    let mut writer = match open_checked(path, expected) {
        Ok(writer) => writer,
        Err(error) => return Body::failed(error),
    };
    let result = match edit {
        HeaderEdit::BaseType(value) => writer.set_base_type(*value),
        HeaderEdit::Description(text) => writer.set_description(text),
        HeaderEdit::CustomFlag { number, text } => {
            writer.set_custom_flag_description(*number, text)
        }
        HeaderEdit::AutoLoad(game) => writer.set_auto_load(*game),
    };
    match result {
        Ok(()) => Body {
            report: MaintReport::Nothing,
            changed: true,
            error: None,
        },
        Err(error) => Body::failed(base_error(&error)),
    }
}

fn compact_body(path: &Path, expected: u32, names: bool, games: bool) -> Body {
    let mut writer = match open_checked(path, expected) {
        Ok(writer) => writer,
        Err(error) => return Body::failed(error),
    };
    let mut names_report = None;
    let mut games_report = None;
    let mut changed = false;
    if names {
        match compact_names(writer) {
            Ok((next, report)) => {
                changed |= report.changed;
                names_report = Some(report);
                writer = next;
            }
            Err(error) => return Body::failed(base_error(&error)),
        }
    }
    if games {
        match compact_games(writer) {
            Ok((_next, report)) => {
                changed = true;
                games_report = Some(report);
            }
            Err(error) => {
                return Body {
                    report: MaintReport::Compact {
                        games: None,
                        names: names_report,
                    },
                    changed,
                    error: Some(base_error(&error)),
                };
            }
        }
    }
    Body {
        report: MaintReport::Compact {
            games: games_report,
            names: names_report,
        },
        changed,
        error: None,
    }
}

fn sort_body(
    path: &Path,
    expected: u32,
    op: (&[mychess_db::si4::SortKey], bool, bool),
    control: &OpenControl,
) -> Body {
    let (keys, reverse, compact_after) = op;
    let writer = match open_checked(path, expected) {
        Ok(writer) => writer,
        Err(error) => return Body::failed(error),
    };
    let mut progress = control.reporter();
    let result = sort_games_controlled(
        writer,
        keys,
        reverse,
        &mut progress,
        &control.token(),
        &mut FailPoint::never(),
    );
    let (writer, report) = match result {
        Ok(done) => done,
        Err(error) => return Body::failed(base_error(&error)),
    };
    if report.interrupted {
        return Body::failed(BaseError::Cancelled);
    }
    let mut compacted = None;
    let mut error = None;
    if compact_after {
        match compact_games(writer) {
            Ok((_next, done)) => compacted = Some(done),
            Err(e) => error = Some(base_error(&e)),
        }
    }
    Body {
        changed: report.moved > 0 || compacted.is_some(),
        report: MaintReport::Sort {
            games: report.games,
            moved: report.moved,
            order: Shared::new(report.order),
            compacted,
        },
        error,
    }
}

fn mark_body(
    path: &Path,
    expected: u32,
    scan: &mychess_db::maintenance::DuplicateScan,
    copy_ratings: bool,
) -> Body {
    let mut writer = match open_checked(path, expected) {
        Ok(writer) => writer,
        Err(error) => return Body::failed(error),
    };
    let (marked, error) = flags_body(&mut writer, &scan.to_delete, GameFlags::DELETE, FlagOp::Set);
    let mut ratings = 0;
    let mut error = error;
    if error.is_none() && copy_ratings && !scan.rating_copies.is_empty() {
        let (changed, result) = writer.set_elo_batch(&scan.rating_copies);
        ratings = changed;
        error = result.err().map(|e| base_error(&e));
    }
    Body {
        changed: marked > 0 || ratings > 0,
        report: MaintReport::Marked {
            marked,
            undeleted: 0,
            ratings,
        },
        error,
    }
}

fn rewrite_body(result: Result<mychess_db::si4::RewriteReport, Si4Error>) -> Body {
    match result {
        Ok(report) => Body {
            changed: report.changed > 0,
            error: report.interrupted.then_some(BaseError::Cancelled),
            report: MaintReport::Rewrite(report),
        },
        Err(error) => Body::failed(base_error(&error)),
    }
}

/// The games of the tournament of `game`, selected on the file.
fn tournament_games(path: &Path, game: u32, control: &OpenControl) -> Result<Vec<u32>, BaseError> {
    let db = Si4Database::open(path).map_err(|e| BaseError::from_si4(&e))?;
    let reference = TournamentReference::from_game(&db, game).map_err(|_| BaseError::Stale)?;
    let mut progress = control.reporter();
    let selection = select_tournament(&db, &reference, true, &mut progress, &control.token())
        .map_err(|_| BaseError::Cancelled)?;
    Ok(selection.games)
}

fn names_body(
    path: &Path,
    expected: u32,
    edit: &mychess_db::si4::NameEdit,
    games: &NameJobGames,
    control: &OpenControl,
) -> Body {
    let numbers: Option<Vec<u32>> = match games {
        NameJobGames::All => None,
        NameJobGames::Set(set) => match set.resolve(expected) {
            Ok(list) => Some(list),
            Err(error) => return Body::failed(error),
        },
        NameJobGames::Tournament(game) => match tournament_games(path, *game, control) {
            Ok(list) => Some(list),
            Err(error) => return Body::failed(error),
        },
    };
    let mut writer = match open_checked(path, expected) {
        Ok(writer) => writer,
        Err(error) => return Body::failed(error),
    };
    let scope = numbers.as_deref().map_or(NameScope::All, NameScope::Games);
    let mut progress = control.reporter();
    match writer.edit_names(edit, scope, &control.token(), &mut progress) {
        Ok(report) => Body {
            changed: report.games_changed > 0 || report.names_added > 0,
            error: report.interrupted.then_some(BaseError::Cancelled),
            report: MaintReport::Names(report),
        },
        Err(NameEditError::Si4(error)) => Body::failed(base_error(&error)),
        Err(refusal) => Body {
            report: MaintReport::NamesRefused(refusal.to_string()),
            changed: false,
            error: None,
        },
    }
}

/// The repair of a database that is not open: the files are the job's own.
fn repair_job(
    job: &MaintJob,
    drop_unreadable: bool,
    open: &[PathBuf],
    control: &OpenControl,
) -> Result<MaintOutcome, BaseError> {
    if control.is_cancelled() {
        return Err(BaseError::Cancelled);
    }
    let path = job.path.as_deref().ok_or(BaseError::Internal)?;
    // The files of an open database are not repaired under its feet.
    if open.iter().any(|p| same_file(p, path)) {
        return Err(BaseError::AlreadyOpen);
    }
    let mut progress = control.reporter();
    let mut out = MaintOutcome::nothing();
    match repair_database(
        path,
        RepairOptions { drop_unreadable },
        &mut progress,
        &control.token(),
    ) {
        Ok(report) => {
            out.backups.clone_from(&report.backups);
            out.changed = !report.interrupted;
            out.error = report.interrupted.then_some(BaseError::Cancelled);
            out.report = MaintReport::Repair(Shared::new(report));
        }
        Err(MaintError::Repair(failed)) => {
            out.backups = failed.backups;
            out.error = Some(base_error(&failed.source));
        }
        Err(error) => out.error = Some(maint_error(&error)),
    }
    Ok(out)
}

/// Runs a job of the list.
#[allow(clippy::too_many_lines)] // one flat match over the operations
pub(crate) fn run(job: &MaintJob, control: &OpenControl) -> Result<MaintOutcome, BaseError> {
    let expected = job.expected_total;
    let token = control.token();
    let mut progress = control.reporter();
    match &job.op {
        MaintOp::Check { .. }
        | MaintOp::Twins { .. }
        | MaintOp::TagList { .. }
        | MaintOp::TagFilter { .. }
        | MaintOp::DropDeleted { .. } => read_job(job, control),
        MaintOp::Repair {
            drop_unreadable,
            open,
        } => repair_job(job, *drop_unreadable, open, control),
        MaintOp::Flags {
            games,
            flag,
            action,
        } => {
            if job.path.is_none() {
                return clip_write(job, control, |clip| {
                    let games = match games.resolve(expected) {
                        Ok(games) => games,
                        Err(error) => return Body::failed(error),
                    };
                    let changed = clip.set_flags(&games, *flag, *action);
                    Body {
                        report: MaintReport::Flags { changed },
                        changed: changed > 0,
                        error: None,
                    }
                });
            }
            file_write(job, control, |path| {
                let games = match games.resolve(expected) {
                    Ok(games) => games,
                    Err(error) => return Body::failed(error),
                };
                let mut writer = match open_checked(path, expected) {
                    Ok(writer) => writer,
                    Err(error) => return Body::failed(error),
                };
                let (changed, error) = flags_body(&mut writer, &games, *flag, *action);
                Body {
                    report: MaintReport::Flags { changed },
                    changed: changed > 0,
                    error,
                }
            })
        }
        MaintOp::UndeleteAll => {
            let all: Vec<u32> = (1..=expected).collect();
            let report = |changed| MaintReport::Marked {
                marked: 0,
                undeleted: changed,
                ratings: 0,
            };
            if job.path.is_none() {
                return clip_write(job, control, |clip| {
                    let changed = clip.set_flags(&all, GameFlags::DELETE, FlagOp::Clear);
                    Body {
                        report: report(changed),
                        changed: changed > 0,
                        error: None,
                    }
                });
            }
            file_write(job, control, |path| {
                let mut writer = match open_checked(path, expected) {
                    Ok(writer) => writer,
                    Err(error) => return Body::failed(error),
                };
                let (changed, error) =
                    flags_body(&mut writer, &all, GameFlags::DELETE, FlagOp::Clear);
                Body {
                    report: report(changed),
                    changed: changed > 0,
                    error,
                }
            })
        }
        MaintOp::MarkTwins { scan, copy_ratings } => {
            if job.path.is_none() {
                return clip_write(job, control, |clip| {
                    let marked = clip.set_flags(&scan.to_delete, GameFlags::DELETE, FlagOp::Set);
                    let ratings = if *copy_ratings {
                        let changes: Vec<_> = scan
                            .rating_copies
                            .iter()
                            .map(
                                |(game, white, black)| mychess_db::spelling::elo::EloChange {
                                    game: *game,
                                    white: *white,
                                    black: *black,
                                },
                            )
                            .collect();
                        clip.apply_elo_changes(&changes)
                    } else {
                        0
                    };
                    Body {
                        report: MaintReport::Marked {
                            marked,
                            undeleted: 0,
                            ratings,
                        },
                        changed: marked > 0 || ratings > 0,
                        error: None,
                    }
                });
            }
            file_write(job, control, |path| {
                mark_body(path, expected, scan, *copy_ratings)
            })
        }
        MaintOp::ShareTags { games: (a, b) } => file_write(job, control, |path| {
            let mut writer = match open_checked(path, expected) {
                Ok(writer) => writer,
                Err(error) => return Body::failed(error),
            };
            match writer.share_tags(*a, *b) {
                Ok(tags) => Body {
                    changed: !tags.is_empty(),
                    report: MaintReport::TagsShared {
                        count: u32::try_from(tags.len()).unwrap_or(u32::MAX),
                    },
                    error: None,
                },
                Err(error) => Body::failed(base_error(&error)),
            }
        }),
        MaintOp::Header(edit) => file_write(job, control, |path| header_body(path, expected, edit)),
        MaintOp::Compact { names, games } => file_write(job, control, |path| {
            compact_body(path, expected, *names, *games)
        }),
        MaintOp::Sort {
            keys,
            reverse,
            compact_after,
        } => file_write(job, control, |path| {
            sort_body(path, expected, (keys, *reverse, *compact_after), control)
        }),
        MaintOp::TagEdit { op, games } => file_write(job, control, |path| {
            let games = match games.resolve(expected) {
                Ok(games) => games,
                Err(error) => return Body::failed(error),
            };
            match open_checked(path, expected) {
                Ok(mut writer) => rewrite_body(writer.edit_tags(&games, op, &token, &mut progress)),
                Err(error) => Body::failed(error),
            }
        }),
        MaintOp::Strip { what, games } => file_write(job, control, |path| {
            let games = match games.resolve(expected) {
                Ok(games) => games,
                Err(error) => return Body::failed(error),
            };
            match open_checked(path, expected) {
                Ok(mut writer) => {
                    rewrite_body(writer.strip_games(&games, *what, &token, &mut progress))
                }
                Err(error) => Body::failed(error),
            }
        }),
        MaintOp::Names { edit, games } => file_write(job, control, |path| {
            names_body(path, expected, edit, games, control)
        }),
        MaintOp::Cleaner(cleaner_job) => file_write(job, control, |path| {
            let run = cleaner::run(cleaner_job, path, expected, control);
            Body {
                changed: run.changed,
                error: run.error,
                report: MaintReport::Cleaner(Shared::new(run.report)),
            }
        }),
    }
    .map(|mut out| {
        if let MaintOp::Cleaner(_) = &job.op {
            out.tree_primed = matches!(&out.report, MaintReport::Cleaner(report)
                if report.record(mychess_db::maintenance::CleanerStep::Tree)
                    .is_some_and(|r| r.status == mychess_db::maintenance::StepStatus::Done));
        }
        out
    })
}

/// The statistics of the database (index only; `None` when it is not an
/// `.si4` or the run was stopped).
fn stats_run(
    source: &SharedSource,
    control: &OpenControl,
) -> Option<mychess_db::maintenance::DatabaseStats> {
    let guard = source.read();
    let BaseSource::Si4(db) = &*guard else {
        return None;
    };
    let mut progress = control.reporter();
    let stats = database_stats(db, &mut progress, &control.token());
    (!stats.interrupted).then_some(stats)
}

/// Games the preview looks at between two looks at the cancel flag.
const PREVIEW_CHECK_EVERY: u64 = 2048;

/// Counts what a confirmation will show.
///
/// The look at the games checks the cancel flag every
/// [`PREVIEW_CHECK_EVERY`] games and reports its progress; `None` when it
/// was stopped.
fn preview_run(
    source: &SharedSource,
    work: &PreviewWork,
    control: &OpenControl,
) -> Option<PreviewResult> {
    let guard = source.read();
    let BaseSource::Si4(db) = &*guard else {
        return None;
    };
    match work {
        PreviewWork::Strip { what, games, total } => {
            let mut affected = 0u32;
            let numbers = games.resolve(*total).ok()?;
            let all = numbers.len() as u64;
            let mut progress = control.reporter();
            for (index, number) in numbers.into_iter().enumerate() {
                if (index as u64).is_multiple_of(PREVIEW_CHECK_EVERY) {
                    if control.is_cancelled() {
                        return None;
                    }
                    progress(index as u64, all);
                }
                let Ok(entry) = db.entry(number) else {
                    continue;
                };
                let wants = match what {
                    mychess_db::si4::StripWhat::Comments => entry.comment_count > 0,
                    mychess_db::si4::StripWhat::Variations => entry.variation_count > 0,
                    mychess_db::si4::StripWhat::Both => {
                        entry.comment_count > 0 || entry.variation_count > 0
                    }
                };
                if wants {
                    affected += 1;
                }
            }
            Some(PreviewResult::Strip(affected))
        }
        PreviewWork::Compact => {
            // The two counts of `mychess-db` have no cancel hook: the flag
            // is looked at between them.
            let mut progress = control.reporter();
            progress(0, 2);
            let stats = compaction_stats(db);
            if control.is_cancelled() {
                return None;
            }
            progress(1, 2);
            let names = name_usage(db);
            (!control.is_cancelled())
                .then_some(PreviewResult::Compact(CompactPreview { stats, names }))
        }
    }
}

/// Compares two games.
fn diff_run(source: &SharedSource, games: (u32, u32)) -> Option<mychess_db::maintenance::GameDiff> {
    let mut db = LockingSource::new(source.clone());
    let a: Game = db.game(games.0).ok()?;
    let b: Game = db.game(games.1).ok()?;
    diff_games(&a, &b).ok()
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    on_panic: impl FnOnce() -> T,
) -> T {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| on_panic())
}

/// Runs an effect of the maintenance tools. Never panics: a task that dies is
/// a failed answer.
pub(crate) async fn perform(effect: MaintenanceEffect) -> Message {
    let message = match effect {
        MaintenanceEffect::Run {
            request,
            job,
            control,
        } => {
            let result = blocking(move || run(&job, &control), || Err(BaseError::Internal)).await;
            MaintenanceMessage::Done {
                request,
                result: Box::new(result),
            }
        }
        MaintenanceEffect::Stats {
            request,
            source,
            control,
        } => {
            let stats = blocking(move || stats_run(&source, &control), || None).await;
            MaintenanceMessage::StatsDone {
                request,
                stats: stats.map(Shared::new),
            }
        }
        MaintenanceEffect::Preview {
            request,
            source,
            work,
            control,
        } => {
            let result = blocking(move || preview_run(&source, &work.0, &control), || None).await;
            MaintenanceMessage::Previewed {
                request,
                result: Box::new(result.map(PreviewDone)),
            }
        }
        MaintenanceEffect::Diff {
            request,
            source,
            games,
        } => {
            let diff = blocking(move || diff_run(&source, games), || None).await;
            MaintenanceMessage::Diffed {
                request,
                diff: diff.map(Shared::new),
            }
        }
        MaintenanceEffect::SharePlan {
            request,
            source,
            games,
        } => {
            let tags = blocking(
                move || {
                    let db = LockingSource::new(source);
                    Some(share_tags_preview(&db, games.0, games.1))
                },
                || None,
            )
            .await;
            MaintenanceMessage::SharePlanned {
                request,
                tags: tags.map(Shared::new),
            }
        }
    };
    Message::Maintenance(message)
}
