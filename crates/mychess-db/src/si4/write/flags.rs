// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_game_flag`) and
// src/index.cpp (`WriteEntries`). Copyright (c) 1999-2003 Shane Hudson; Scid
// vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Flags in bulk (`sc_game flag <flag> filter|all`, decision D3505 of
//! `docs/specs/m7-1-manutencao.md`): the same bytes as
//! [`Si4Writer::set_flag`], with a single `fsync` at the end.

use std::io::{Seek, SeekFrom, Write};

use super::{FlagOp, Si4Writer, entry_position, flags_of, io_error};
use crate::si4::entry::GameFlags;
use crate::si4::error::{Result, Si4Error};

/// Offset of `Length_High` (custom flags in bits 0-5); the flag word follows.
const FLAGS_OFFSET: usize = 6;

impl Si4Writer {
    /// Applies `op` to `flag` in every game of `games` (`sc_game flag`).
    ///
    /// `flag` must be made of editable flags ([`GameFlags::EDITABLE`]); it may
    /// have several bits. The list is sorted and deduplicated first, so a
    /// repeated number counts once (also for [`FlagOp::Toggle`]); number `0` is
    /// ignored, as in the Scid; any other number out of range fails **before**
    /// a byte is written. Only bytes 6-8 of an entry are written, and only if
    /// the flags change; the other 44 bytes stay as they are. One `fsync` at
    /// the end. Returns how many entries changed.
    ///
    /// The batch is **not atomic**: if a write fails midway, the entries
    /// already written keep their new flags and the others their old ones
    /// (each entry stays valid, see [`Si4Writer::set_flags_batch_counted`]).
    ///
    /// # Errors
    ///
    /// [`Si4Error::NotEditableFlag`], [`Si4Error::InvalidGameNumber`] or an
    /// I/O error (see [`Si4Writer::set_flags_batch_counted`] for the count in
    /// that case).
    pub fn set_flags_batch(&mut self, games: &[u32], flag: GameFlags, op: FlagOp) -> Result<u32> {
        let (changed, result) = self.set_flags_batch_counted(games, flag, op);
        result.map(|()| changed)
    }

    /// Like [`Si4Writer::set_flags_batch`], but always returns how many
    /// entries changed on disk, even when it stops with an I/O error (what was
    /// written stays valid: each entry is complete).
    pub fn set_flags_batch_counted(
        &mut self,
        games: &[u32],
        flag: GameFlags,
        op: FlagOp,
    ) -> (u32, Result<()>) {
        if flag.is_empty() || !GameFlags::EDITABLE.contains(flag) {
            return (0, Err(Si4Error::NotEditableFlag));
        }
        if let Err(error) = self.check_alive() {
            return (0, Err(error));
        }
        let len = self.len();
        let mut numbers: Vec<u32> = games.iter().copied().filter(|&n| n != 0).collect();
        numbers.sort_unstable();
        numbers.dedup();
        if let Some(&number) = numbers.iter().find(|&&n| n > len) {
            return (0, Err(Si4Error::InvalidGameNumber { number, len }));
        }
        let mut changed = 0u32;
        let mut result = Ok(());
        for number in numbers {
            match self.set_flags_unsynced(number, flag, op) {
                Ok(true) => changed = changed.saturating_add(1),
                Ok(false) => {}
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        let synced = if changed > 0 {
            self.sync_index()
        } else {
            Ok(())
        };
        (changed, result.and(synced))
    }

    fn set_flags_unsynced(&mut self, number: u32, flag: GameFlags, op: FlagOp) -> Result<bool> {
        let bytes = self.read_entry(number)?;
        let old = flags_of(&bytes);
        let new = match op {
            FlagOp::Set => old | flag,
            FlagOp::Clear => old & !flag,
            FlagOp::Toggle => old ^ flag,
        };
        if new == old {
            return Ok(false);
        }
        // Bytes 6-8 in one write: custom flags (bits 0-5 of byte 6; the top
        // two bits stay) and the 16-bit flag word.
        let low = u16::try_from(new.bits() & 0xFFFF)
            .unwrap_or(0)
            .to_be_bytes();
        let custom = u8::try_from((new.bits() >> 16) & 0x3F).unwrap_or(0);
        let byte6 = bytes.get(FLAGS_OFFSET).copied().unwrap_or(0);
        let patch = [(byte6 & 0xC0) | custom, low[0], low[1]];
        let position = entry_position(number) + FLAGS_OFFSET as u64;
        let path = &self.index_path;
        let written = self
            .index_file
            .seek(SeekFrom::Start(position))
            .and_then(|_| self.index_file.write_all(&patch))
            .map_err(io_error(path));
        if written.is_err() {
            // The three bytes may be torn.
            self.poisoned = true;
        }
        written.map(|()| true)
    }
}
