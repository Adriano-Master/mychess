// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_eco_base`) and
// src/index.cpp (`WriteEntries`). Copyright (c) 1999-2003 Shane Hudson; Scid
// vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! ECO writing of [`Si4Writer`]: the two bytes of the code in the index entry
//! (`docs/formatos/si4.md`, offset 23) and the reads the classification of a
//! database needs (`docs/specs/m5-7-eco-e-novidade.md`, D1186 and D1187).

use std::io::{self, Read, Seek, SeekFrom, Write};

use mychess_core::Eco;

use super::{Si4Writer, corrupt_entry, entry_position, io_error};
use crate::si4::entry::IndexEntry;
use crate::si4::error::{Result, Si4Error, Si4File};

/// Offset of the two ECO bytes inside the 47-byte entry.
const ECO_OFFSET: usize = 23;

/// The stored number of an optional code (`0` = none).
fn code_of(eco: Option<Eco>) -> u16 {
    eco.map_or(0, Eco::to_scid_code)
}

impl Si4Writer {
    /// Sets the ECO code of game `number` (`None` = no ECO). Only bytes 23-24
    /// of the 47-byte entry are written, and only if the value changes; the
    /// other 45 bytes stay as they are. Returns whether it changed. An I/O
    /// error while writing leaves the two bytes in an unknown state, so the
    /// writer becomes unusable (decision D322).
    pub fn set_eco(&mut self, number: u32, eco: Option<Eco>) -> Result<bool> {
        self.check_alive()?;
        let changed = self.set_eco_unsynced(number, eco)?;
        if changed {
            self.sync_index()?;
        }
        Ok(changed)
    }

    /// Like [`Si4Writer::set_eco`] for many games with one `fsync` at the end.
    /// Game numbers must be valid; a failure stops at that game and keeps
    /// what was already written (each entry is complete on its own). Returns
    /// how many entries changed. A game listed twice takes the last value.
    /// Use [`Si4Writer::set_eco_batch_counted`] to know how many entries were
    /// written when it fails.
    pub fn set_eco_batch(&mut self, changes: &[(u32, Option<Eco>)]) -> Result<u32> {
        let (changed, result) = self.set_eco_batch_counted(changes);
        result.map(|()| changed)
    }

    /// Like [`Si4Writer::set_eco_batch`], but always returns how many
    /// entries were changed on disk, even when it stops with an error.
    pub fn set_eco_batch_counted(&mut self, changes: &[(u32, Option<Eco>)]) -> (u32, Result<()>) {
        if let Err(error) = self.check_alive() {
            return (0, Err(error));
        }
        let mut changed = 0u32;
        let mut result = Ok(());
        for &(number, eco) in changes {
            match self.set_eco_unsynced(number, eco) {
                Ok(true) => changed = changed.saturating_add(1),
                Ok(false) => {}
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        // Whatever was written is made durable even when a later game failed.
        let synced = if changed > 0 {
            self.sync_index()
        } else {
            Ok(())
        };
        (changed, result.and(synced))
    }

    /// Writes the two ECO bytes of the entry if they differ, without
    /// `fsync`. A write failure poisons the writer.
    fn set_eco_unsynced(&mut self, number: u32, eco: Option<Eco>) -> Result<bool> {
        let new = code_of(eco).to_be_bytes();
        let bytes = self.read_entry(number)?;
        let Some(slot) = bytes.get(ECO_OFFSET..ECO_OFFSET + 2) else {
            return Ok(false);
        };
        if *slot == new {
            return Ok(false);
        }
        let position = entry_position(number) + ECO_OFFSET as u64;
        let path = &self.index_path;
        let written = self
            .index_file
            .seek(SeekFrom::Start(position))
            .and_then(|_| self.index_file.write_all(&new))
            .map_err(io_error(path));
        if written.is_err() {
            self.poisoned = true;
        }
        written.map(|()| true)
    }

    /// `fsync` of the index; a failure poisons the writer.
    pub(super) fn sync_index(&mut self) -> Result<()> {
        let path = &self.index_path;
        let synced = self.index_file.sync_all().map_err(io_error(path));
        if synced.is_err() {
            self.poisoned = true;
        }
        synced
    }

    /// The decoded index entry of game `number`, read from the file.
    pub(crate) fn entry_at(&mut self, number: u32) -> Result<IndexEntry> {
        let bytes = self.read_entry(number)?;
        IndexEntry::parse_lenient(&bytes)
            .map(|(entry, _)| entry)
            .map_err(|e| corrupt_entry(number, e))
    }

    /// Reads the record of game `number` described by `entry` into `buf`
    /// (cleared first), so a scan reuses one buffer.
    pub(crate) fn record_into(
        &mut self,
        number: u32,
        entry: &IndexEntry,
        buf: &mut Vec<u8>,
    ) -> Result<()> {
        let corrupt = |reason: &'static str| Si4Error::CorruptRecord {
            game: number,
            offset: u64::from(entry.offset),
            reason,
        };
        if entry.length == 0 {
            return Err(corrupt("empty record"));
        }
        let truncated = || Si4Error::Truncated {
            file: Si4File::Games,
            offset: u64::from(entry.offset),
            need: u64::from(entry.length),
        };
        buf.clear();
        let path = &self.games_path;
        self.games_file
            .seek(SeekFrom::Start(u64::from(entry.offset)))
            .map_err(io_error(path))?;
        // `length` has at most 17 bits, so the buffer stays small.
        let read = (&mut self.games_file)
            .take(u64::from(entry.length))
            .read_to_end(buf)
            .map_err(|source: io::Error| Si4Error::Io {
                path: path.clone(),
                source,
            })?;
        if read as u64 != u64::from(entry.length) {
            return Err(truncated());
        }
        Ok(())
    }
}
