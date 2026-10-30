//! The blocking work of the advanced searches (what `perform` runs on a
//! blocking thread; `update` never calls it).

use std::path::Path;

use mychess_db::{
    AdvancedQuery, Filter, SavedAdvanced, SavedSearchError, delete_advanced, import_material_sso,
    list_advanced, load_advanced, save_advanced, search_board, search_material, search_moves,
};

use super::message::{
    AdvancedDone, SavedCommand, SavedEntry, SavedFailure, SavedOutcome, SearchFailure,
};
use crate::bases::{OpenControl, SharedSource};
use crate::tree::LockingSource;

/// Runs one search over `source` on a copy of `filter`. The database is
/// locked for each record, never for the whole scan, so the list and the
/// tree can be drawn meanwhile. A stop leaves `report.interrupted` set and
/// the filter as it was (D3202).
pub(crate) fn run_search(
    source: SharedSource,
    mut filter: Filter,
    query: &AdvancedQuery,
    control: &OpenControl,
) -> Result<AdvancedDone, SearchFailure> {
    let mut db = LockingSource::new(source);
    let mut progress = control.reporter();
    let cancel = control.token();
    let report = match query {
        AdvancedQuery::Position(q) => search_board(&mut db, q, &mut filter, &mut progress, &cancel),
        AdvancedQuery::Material(q) => {
            search_material(&mut db, q, &mut filter, &mut progress, &cancel)
        }
        AdvancedQuery::Moves(q) => search_moves(&mut db, q, &mut filter, &mut progress, &cancel),
        _ => return Err(SearchFailure::Internal),
    }
    .map_err(SearchFailure::Search)?;
    Ok(AdvancedDone { filter, report })
}

/// Runs one operation on the saved searches of `dir`.
pub(crate) fn run_saved(dir: &Path, command: SavedCommand) -> Result<SavedOutcome, SavedFailure> {
    let fail = |e: SavedSearchError| {
        tracing::warn!(error = %e, "saved search operation failed");
        SavedFailure::from_error(&e)
    };
    match command {
        SavedCommand::List => {
            let list = list_advanced(dir).map_err(fail)?;
            Ok(SavedOutcome::Listed {
                entries: list
                    .entries
                    .into_iter()
                    .map(|e| SavedEntry {
                        name: e.name,
                        created: e.created,
                    })
                    .collect(),
                skipped: list.skipped.len(),
            })
        }
        SavedCommand::Save(saved) => {
            let saved: SavedAdvanced = *saved;
            save_advanced(dir, &saved).map_err(fail)?;
            Ok(SavedOutcome::Saved(saved.name))
        }
        SavedCommand::Load(name) => {
            let saved = load_advanced(dir, &name).map_err(fail)?;
            Ok(SavedOutcome::Loaded(Box::new(saved.query)))
        }
        SavedCommand::Delete(name) => {
            let found = delete_advanced(dir, &name).map_err(fail)?;
            Ok(SavedOutcome::Deleted(name, found))
        }
        SavedCommand::ImportSso(path) => {
            let report = import_material_sso(&path).map_err(fail)?;
            Ok(SavedOutcome::Imported {
                search: Box::new(report.search),
                warnings: report
                    .warnings
                    .len()
                    .saturating_add(usize::try_from(report.warnings_dropped).unwrap_or(usize::MAX)),
            })
        }
    }
}
