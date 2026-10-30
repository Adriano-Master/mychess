//! `repair_database` as the maintenance API (spec M7-1, rule 43): the
//! implementation lives next to the other journaled rewrites in
//! [`crate::si4::repair`]; this wrapper gives it the error type of the
//! maintenance functions.

use std::path::Path;

use crate::progress::{CancelToken, ProgressSink};
use crate::si4::{RepairOptions, RepairReport};

use super::MaintError;

/// Repairs a damaged SI4 database; see [`crate::si4::repair`].
///
/// # Errors
///
/// The [`Si4Error`](crate::si4::Si4Error) of the repair, as
/// [`MaintError::Si4`]; when backups were already made it is
/// [`MaintError::Repair`] instead, which lists them. The original files are
/// intact after an error.
pub fn repair_database(
    base: &Path,
    options: RepairOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<RepairReport, MaintError> {
    crate::si4::repair_database(base, options, progress, cancel).map_err(|error| {
        if error.backups.is_empty() {
            MaintError::Si4(error.source)
        } else {
            MaintError::Repair(error)
        }
    })
}
