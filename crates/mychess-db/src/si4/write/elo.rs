// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_ratings`) and
// src/index.cpp (`WriteEntries`).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Copyright notices of src/index.cpp: Copyright (c) 1999-2002 Shane Hudson;
// Copyright (c) 2006-2009 Pascal Georges; Copyright (c) 2010 Steven Atkinson.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Rating writing of [`Si4Writer`]: the 16 bits of each side in the index
//! entry (`docs/formatos/si4.md`, offsets 29 and 31: rating type in the high
//! 4 bits, Elo in the low 12).

use std::io::{Seek, SeekFrom, Write};

use mychess_core::MAX_ELO;

use super::{Si4Writer, entry_position, io_error};
use crate::si4::error::Result;

/// Offset of the two rating words inside the 47-byte entry.
const ELO_OFFSET: usize = 29;

/// An optional new `(Elo, rating type)` for one side of a game.
pub type SideRating = Option<(u16, u8)>;

pub(super) fn word(elo: u16, kind: u8) -> [u8; 2] {
    ((u16::from(kind & 0xF) << 12) | elo.min(MAX_ELO)).to_be_bytes()
}

impl Si4Writer {
    /// Sets the ratings of many games with one `fsync` at the end. Each item
    /// is `(game number, White, Black)`; a side that is `None` is left as it
    /// is, a side that is `Some((elo, type))` gets the Elo (limited to 4000)
    /// and the rating type (0 = `Elo`, up to 15). Only the four bytes 29-32
    /// of an entry are written, and only if they change; the other 43 bytes
    /// stay. Returns how many entries changed on disk (also when it stops
    /// with an error: what was written stays valid, each entry being
    /// complete) and the result.
    pub fn set_elo_batch(
        &mut self,
        changes: &[(u32, SideRating, SideRating)],
    ) -> (u32, Result<()>) {
        if let Err(error) = self.check_alive() {
            return (0, Err(error));
        }
        let mut changed = 0u32;
        let mut result = Ok(());
        for &(number, white, black) in changes {
            match self.set_elo_unsynced(number, white, black) {
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

    fn set_elo_unsynced(
        &mut self,
        number: u32,
        white: SideRating,
        black: SideRating,
    ) -> Result<bool> {
        let bytes = self.read_entry(number)?;
        let Some(old) = bytes.get(ELO_OFFSET..ELO_OFFSET + 4) else {
            return Ok(false);
        };
        let mut new = [0u8; 4];
        new.copy_from_slice(old);
        if let Some((elo, kind)) = white {
            new[..2].copy_from_slice(&word(elo, kind));
        }
        if let Some((elo, kind)) = black {
            new[2..].copy_from_slice(&word(elo, kind));
        }
        if new == *old {
            return Ok(false);
        }
        let position = entry_position(number) + ELO_OFFSET as u64;
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
}
