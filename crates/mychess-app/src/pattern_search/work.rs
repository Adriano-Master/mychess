//! The blocking work of the search for patterns (what `perform` runs on a
//! blocking thread; `update` never calls it).

use std::path::Path;

use mychess_core::Game;
use mychess_db::pattern::model::Pattern;
use mychess_db::{
    Filter, FilterOp, PatternHit, PatternScanError, PatternScanLimits, delete_pattern,
    delete_pattern_file, export_pattern, hits_in_game_with, import_pattern, list_patterns,
    load_pattern, load_pattern_named, save_pattern, search_pattern_with,
};

use super::message::{
    PatternCommand, PatternDone, PatternFailure, PatternIssue, SavedFailure, SavedOutcome,
    SavedPattern,
};
use crate::bases::{OpenControl, SharedSource};
use crate::tree::LockingSource;

/// Runs one search over `source` on a copy of `filter`. The database is
/// locked for each record, never for the whole scan, so the list and the
/// tree can be drawn meanwhile. A stop leaves `report.interrupted` set and
/// the filter as it was.
pub(crate) fn run_search(
    source: SharedSource,
    mut filter: Filter,
    pattern: &Pattern,
    op: FilterOp,
    control: &OpenControl,
) -> Result<PatternDone, PatternFailure> {
    let mut db = LockingSource::new(source);
    let mut progress = control.reporter();
    let cancel = control.token();
    let report = search_pattern_with(
        &mut db,
        pattern,
        op,
        &mut filter,
        &PatternScanLimits::default(),
        &mut progress,
        &cancel,
    )
    .map_err(|e| match e {
        PatternScanError::Pattern(e) => PatternFailure::Pattern(PatternIssue::from_error(&e)),
        PatternScanError::Search(_) => PatternFailure::FilterSize,
        _ => PatternFailure::Internal,
    })?;
    Ok(PatternDone { filter, report })
}

/// The occurrences of `pattern` in `game`, with the same limits as a scan.
pub(crate) fn run_hits(game: &Game, pattern: &Pattern) -> Result<Vec<PatternHit>, PatternFailure> {
    hits_in_game_with(pattern, game, &PatternScanLimits::default())
        .map_err(|e| PatternFailure::Pattern(PatternIssue::from_error(&e)))
}

/// Runs one operation on the patterns of `dir`.
pub(crate) fn run_saved(dir: &Path, command: PatternCommand) -> Result<SavedOutcome, SavedFailure> {
    let fail = |e: mychess_db::PatternError| {
        tracing::warn!(error = %e, "pattern file operation failed");
        SavedFailure::from_error(&e)
    };
    match command {
        PatternCommand::List => {
            let list = list_patterns(dir).map_err(fail)?;
            Ok(SavedOutcome::Listed(
                list.into_iter()
                    .map(|s| SavedPattern {
                        name: s.name,
                        description: s.description,
                        created: s.created,
                        path: s.path,
                        damaged: s.broken.is_some(),
                    })
                    .collect(),
            ))
        }
        PatternCommand::Save(file) => {
            save_pattern(dir, &file).map_err(fail)?;
            Ok(SavedOutcome::Saved(file.pattern.name.clone()))
        }
        PatternCommand::Load(name) => Ok(SavedOutcome::Loaded(Box::new(
            load_pattern_named(dir, &name).map_err(fail)?,
        ))),
        PatternCommand::Delete(name) => {
            delete_pattern(dir, &name).map_err(fail)?;
            Ok(SavedOutcome::Deleted(name))
        }
        PatternCommand::DeleteFile(path) => {
            delete_pattern_file(dir, &path).map_err(fail)?;
            Ok(SavedOutcome::Deleted(
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        PatternCommand::Import(source) => {
            let stored = import_pattern(dir, &source).map_err(fail)?;
            Ok(SavedOutcome::Imported(Box::new(
                load_pattern(&stored).map_err(fail)?,
            )))
        }
        PatternCommand::Export { path, file } => {
            export_pattern(&path, &file).map_err(fail)?;
            Ok(SavedOutcome::Exported)
        }
    }
}
