//! [`GameSource`]: the read interface shared by every kind of database
//! (decision D351 of `docs/specs/m3-3-filtro-busca-importacao.md`).
//!
//! The header search, the quick text filter, the player statistics and the
//! PGN export read a database through this trait, so they work the same on an
//! SI4 database ([`Si4Database`]) and on a PGN file opened as a database
//! ([`PgnDatabase`](crate::PgnDatabase)). The trait has no write operation:
//! writing is a property of [`Si4Writer`](crate::si4::Si4Writer) only.

use std::io::{Read, Seek};

use mychess_core::Game;

use crate::si4::{Header, IndexEntry, NameBase, RawGame, Result, Si4Database};

/// A database that can be read game by game.
///
/// Game numbers are 1-based (`1..=len()`), as in the Scid. The methods that
/// only look at the index and the name base take `&self`; the ones that read
/// a game record take `&mut self`, because the record store keeps a buffer
/// (and, for a file, a position).
pub trait GameSource {
    /// The `.si4` header (type, description, custom flag names).
    fn header(&self) -> &Header;

    /// The number of games.
    fn len(&self) -> u32;

    /// Whether there are no games.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The index entry of game `n`.
    ///
    /// # Errors
    ///
    /// The game number is out of range or the entry is corrupt.
    fn entry(&self, n: u32) -> Result<IndexEntry>;

    /// The name base.
    fn names(&self) -> &NameBase;

    /// The bytes of the record of game `n`, as stored.
    ///
    /// # Errors
    ///
    /// The game number is out of range or the record cannot be read.
    fn record_bytes(&mut self, n: u32) -> Result<&[u8]>;

    /// The record of game `n` decoded into a raw tree, without checking that
    /// the moves are legal.
    ///
    /// # Errors
    ///
    /// The game number is out of range or the record is corrupt.
    fn raw_game(&mut self, n: u32) -> Result<RawGame>;

    /// Whether this is an SI4 database, the only kind that can have a tree
    /// cache file (`.stc`, decision D1055 of `docs/specs/m5-3-cache-stc.md`).
    /// The default is `false`; wrappers around an SI4 database must forward
    /// it.
    fn is_scid_database(&self) -> bool {
        false
    }

    /// Game `n` as a [`Game`] of the core.
    ///
    /// # Errors
    ///
    /// The game number is out of range or the game cannot be decoded.
    fn game(&mut self, n: u32) -> Result<Game>;
}

impl<R: Read + Seek> GameSource for Si4Database<R> {
    fn is_scid_database(&self) -> bool {
        true
    }

    fn header(&self) -> &Header {
        Si4Database::header(self)
    }

    fn len(&self) -> u32 {
        Si4Database::len(self)
    }

    fn entry(&self, n: u32) -> Result<IndexEntry> {
        Si4Database::entry(self, n)
    }

    fn names(&self) -> &NameBase {
        Si4Database::names(self)
    }

    fn record_bytes(&mut self, n: u32) -> Result<&[u8]> {
        Si4Database::record_bytes(self, n)
    }

    fn raw_game(&mut self, n: u32) -> Result<RawGame> {
        Si4Database::raw_game(self, n)
    }

    fn game(&mut self, n: u32) -> Result<Game> {
        Si4Database::game(self, n)
    }
}
