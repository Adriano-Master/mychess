//! The data behind a slot: an enum over the kinds of database, and the
//! shared handle that lets an effect read a game off the interface thread
//! (spec M3-4, D501).
//!
//! The application only knows the [`GameSource`] trait of `mychess-db` and
//! this enum; there is no `dyn` anywhere.

use std::fs::File;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};

use mychess_core::Game;
use mychess_db::si4::{Header, IndexEntry, NameBase, RawGame, Result as DbResult, Si4Database};
use mychess_db::{GameSource, PgnDatabase};

use super::clipbase::Clipbase;

/// Which kind of database a slot holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseKind {
    /// A Scid database (`.si4`), opened read-only in this slice.
    Si4,
    /// A PGN or `.pgn.gz` file loaded in memory (type 3, read-only).
    Pgn,
    /// The Clipbase (type 2, in memory).
    Clipbase,
}

/// The database of a slot.
pub enum BaseSource {
    /// A Scid database on disk.
    Si4(Box<Si4Database<File>>),
    /// A PGN file in memory.
    Pgn(Box<PgnDatabase>),
    /// The Clipbase.
    Clipbase(Box<Clipbase>),
}

impl std::fmt::Debug for BaseSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BaseSource")
            .field("kind", &self.kind())
            .field("games", &GameSource::len(self))
            .finish_non_exhaustive()
    }
}

impl BaseSource {
    /// The kind of database.
    pub fn kind(&self) -> BaseKind {
        match self {
            BaseSource::Si4(_) => BaseKind::Si4,
            BaseSource::Pgn(_) => BaseKind::Pgn,
            BaseSource::Clipbase(_) => BaseKind::Clipbase,
        }
    }

    /// Whether games can never be added, replaced or flagged: a PGN file
    /// loaded in memory. An SI4 database is editable unless its file is
    /// read-only or the user marked it so (slice 4c, D560); the Clipbase is
    /// always editable.
    pub fn is_read_only(&self) -> bool {
        matches!(self, BaseSource::Pgn(_))
    }

    /// The Clipbase, if this is it.
    pub(crate) fn clipbase(&self) -> Option<&Clipbase> {
        match self {
            BaseSource::Clipbase(clip) => Some(clip),
            _ => None,
        }
    }

    /// The Clipbase, if this is it.
    pub(crate) fn clipbase_mut(&mut self) -> Option<&mut Clipbase> {
        match self {
            BaseSource::Clipbase(clip) => Some(clip),
            _ => None,
        }
    }
}

impl GameSource for BaseSource {
    fn is_scid_database(&self) -> bool {
        matches!(self, BaseSource::Si4(_))
    }

    fn header(&self) -> &Header {
        match self {
            BaseSource::Si4(db) => GameSource::header(db.as_ref()),
            BaseSource::Pgn(db) => GameSource::header(db.as_ref()),
            BaseSource::Clipbase(db) => GameSource::header(db.as_ref()),
        }
    }

    fn len(&self) -> u32 {
        match self {
            BaseSource::Si4(db) => GameSource::len(db.as_ref()),
            BaseSource::Pgn(db) => GameSource::len(db.as_ref()),
            BaseSource::Clipbase(db) => GameSource::len(db.as_ref()),
        }
    }

    fn entry(&self, n: u32) -> DbResult<IndexEntry> {
        match self {
            BaseSource::Si4(db) => GameSource::entry(db.as_ref(), n),
            BaseSource::Pgn(db) => GameSource::entry(db.as_ref(), n),
            BaseSource::Clipbase(db) => GameSource::entry(db.as_ref(), n),
        }
    }

    fn names(&self) -> &NameBase {
        match self {
            BaseSource::Si4(db) => GameSource::names(db.as_ref()),
            BaseSource::Pgn(db) => GameSource::names(db.as_ref()),
            BaseSource::Clipbase(db) => GameSource::names(db.as_ref()),
        }
    }

    fn record_bytes(&mut self, n: u32) -> DbResult<&[u8]> {
        match self {
            BaseSource::Si4(db) => GameSource::record_bytes(db.as_mut(), n),
            BaseSource::Pgn(db) => GameSource::record_bytes(db.as_mut(), n),
            BaseSource::Clipbase(db) => GameSource::record_bytes(db.as_mut(), n),
        }
    }

    fn raw_game(&mut self, n: u32) -> DbResult<RawGame> {
        match self {
            BaseSource::Si4(db) => GameSource::raw_game(db.as_mut(), n),
            BaseSource::Pgn(db) => GameSource::raw_game(db.as_mut(), n),
            BaseSource::Clipbase(db) => GameSource::raw_game(db.as_mut(), n),
        }
    }

    fn game(&mut self, n: u32) -> DbResult<Game> {
        match self {
            BaseSource::Si4(db) => GameSource::game(db.as_mut(), n),
            BaseSource::Pgn(db) => GameSource::game(db.as_mut(), n),
            BaseSource::Clipbase(db) => GameSource::game(db.as_mut(), n),
        }
    }
}

/// A database shared between the state and the effects that read it.
///
/// Reading a record needs `&mut` (the record store keeps a buffer and, for
/// a file, a position), and an [`Effect`](crate::Effect) must own what it
/// works on, so the database sits behind a read-write lock. This is the one
/// place of the application layer with shared mutable state, on purpose:
/// databases are large and cannot be copied into an effect. Write locks are
/// held for one record at a time; read locks may last a whole search (they
/// do not exclude each other). No code holds two sources at once.
#[derive(Clone)]
pub struct SharedSource(Arc<RwLock<BaseSource>>);

impl std::fmt::Debug for SharedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedSource(..)")
    }
}

impl SharedSource {
    /// Shares `source`.
    pub fn new(source: BaseSource) -> SharedSource {
        SharedSource(Arc::new(RwLock::new(source)))
    }

    /// Locks the database, for reading (the list code of the front-end
    /// does it through [`Bases::list_page`](super::Bases::list_page); the
    /// lock is public only so [`list_page`](super::list_page) can be called
    /// on a source with a filter of the caller). Only `update` may change
    /// the Clipbase. A lock poisoned by a panic elsewhere is taken
    /// anyway: the data is only read or appended to, never left half
    /// changed by a failed step.
    pub fn lock(&self) -> RwLockWriteGuard<'_, BaseSource> {
        self.0.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Like [`SharedSource::read`] but gives up at once if a writer holds or
    /// waits for the lock: `view` must never wait (D570).
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, BaseSource>> {
        match self.0.try_read() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    /// Locks the database for reading only: the index and the names, which
    /// is all a list page, a search or a sort needs. Several readers can
    /// hold it at once, so a long search does not freeze the list drawn by
    /// `view` (D559). Reading a game record needs [`SharedSource::lock`].
    pub fn read(&self) -> RwLockReadGuard<'_, BaseSource> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }
}

impl PartialEq for SharedSource {
    fn eq(&self, other: &SharedSource) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedSource {}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_db::progress::NoProgress;
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, OpenOptions};
    use std::path::Path;

    #[test]
    fn only_the_si4_variant_is_a_scid_database() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join("t");
        drop(Si4Writer::create(&base, 1).expect("create"));
        let si4 = Si4Database::open(&base).expect("open");
        assert!(GameSource::is_scid_database(&BaseSource::Si4(Box::new(
            si4
        ))));

        let (pgn, _) = PgnDatabase::from_reader(
            &b""[..],
            Path::new("empty.pgn"),
            &OpenOptions::default(),
            &mut NoProgress,
            &CancelToken::new(),
        )
        .expect("pgn");
        assert!(!GameSource::is_scid_database(&BaseSource::Pgn(Box::new(
            pgn
        ))));

        let clip = Clipbase::new(100).expect("clipbase");
        assert!(!GameSource::is_scid_database(&BaseSource::Clipbase(
            Box::new(clip)
        )));
    }
}
