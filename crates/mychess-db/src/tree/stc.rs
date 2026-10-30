// Derived from Scid vs PC 4.27, src/tree.cpp, src/tree.h (`TreeCache`),
// src/filter.cpp (`packBytemap`, `unpackBytemap`), src/movelist.cpp
// (`writeSimpleMove`) and tcl/windows/tree.tcl (`::tree::prime*`). Copyright
// (c) 1999-2002 Shane Hudson; Scid vs PC contributors. Scid is distributed
// under the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The tree cache file (`.stc`, `docs/specs/m5-3-cache-stc.md`, decisions
//! D1040 to D1054): reading, writing and priming the cache of the opening
//! tree.
//!
//! [`StcCache`] implements the [`TreeCache`](super::TreeCache) hook of the
//! tree engine with the two layers of the Scid: a main cache that lives in
//! the `.stc` file next to the database and a small memory-only backup cache.
//! The file layout is documented in [`mod@format`] and in `docs/formatos/stc.md`,
//! and is byte compatible with the Scid's; the compressed filters use [`pack_bytemap`] and [`unpack_bytemap`].
//!
//! The priming functions ([`prime_with_lines`], [`prime_with_game`],
//! [`prime_with_filter`]) fill the cache by running full tree searches.

mod bytemap;
mod cache;
mod extras;
pub mod format;
mod policy;
mod prime;

use std::path::PathBuf;

use thiserror::Error;

pub use bytemap::{pack_bytemap, unpack_bytemap};
pub use cache::{
    BACKUP_CACHE_SIZE, DEFAULT_CACHE_SIZE, MAX_CACHE_SIZE, MAX_STC_BYTES, MAX_WRITE_BYTES,
    STC_SUFFIX, StcCache, remove_stc, stc_path,
};
pub(crate) use cache::{create_temp, resolve_target};
pub use policy::Policy;
pub use prime::{
    PrimeError, PrimeReport, STANDARD_LINES, prime_with_filter, prime_with_game, prime_with_lines,
};

/// Errors of the cache file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StcError {
    /// A file operation failed.
    #[error("i/o error on {path}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// The data is not a valid cache file or compressed filter (wrong magic,
    /// truncated, out-of-range field, incoherent length).
    #[error("corrupt tree cache: {reason}")]
    Corrupt {
        /// What is wrong.
        reason: &'static str,
    },
    /// The file is bigger than [`MAX_STC_BYTES`].
    #[error("tree cache file too large ({len} bytes)")]
    TooLarge {
        /// The size of the file.
        len: u64,
    },
}

impl StcError {
    pub(crate) fn corrupt(reason: &'static str) -> StcError {
        StcError::Corrupt { reason }
    }
}
