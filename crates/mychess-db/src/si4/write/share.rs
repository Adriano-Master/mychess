// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_game_tags_share`,
// 10098-10320) and src/index.cpp (`WriteEntries`). Copyright (c) 1999-2004
// Shane Hudson; Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Sharing tags between two twin games (`sc_game tags share`, decision D3514
//! of `docs/specs/m7-1-manutencao.md`): the date when one is a prefix of the
//! other, the event and the round when one side is empty or `?`, and the
//! ratings when one side is 0.

use std::fmt::Write as _;
use std::io::{Seek, SeekFrom, Write};

use mychess_core::PartialDate;

use super::{Si4Writer, entry_position, io_error};
use crate::si4::entry::IndexEntry;
use crate::si4::error::Result;
use crate::si4::header::ENTRY_LEN;
use crate::si4::names::NameKind;

/// The tag that changes in a [`SharedTag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharedField {
    /// `Date`.
    Date,
    /// `Event`.
    Event,
    /// `Round`.
    Round,
    /// `WhiteElo`.
    WhiteElo,
    /// `BlackElo`.
    BlackElo,
}

/// One change proposed or made by `share_tags`: game `game` gets `new` in
/// place of `old` (texts as the Scid shows them: `2020`, `Aberto`, `2500`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedTag {
    /// The game that changes (base 1).
    pub game: u32,
    /// The tag.
    pub field: SharedField,
    /// The value now.
    pub old: String,
    /// The value after the change.
    pub new: String,
}

/// How the name behind an ID reads for the event and round rule.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// Empty or `?`: can receive.
    Empty,
    /// A real name: can give.
    Known,
    /// The ID is not in the name base: neither gives nor receives.
    Missing,
}

fn fill_of(name: Option<&[u8]>) -> Fill {
    match name {
        None => Fill::Missing,
        Some(b"" | b"?") => Fill::Empty,
        Some(_) => Fill::Known,
    }
}

/// The date as `strTrimDate` shows it: unknown trailing parts are cut, an
/// unknown year gives the empty text.
fn trimmed_date(entry: &IndexEntry) -> String {
    let date = entry.date;
    let Some(year) = date.year() else {
        return String::new();
    };
    let mut text = format!("{year:04}");
    if let Some(month) = date.month() {
        write!(text, ".{month:02}").ok();
        if let Some(day) = date.day() {
            write!(text, ".{day:02}").ok();
        }
    }
    text
}

/// The fields one game receives. Only these fields are written, on top of the
/// bytes already in the file (a lenient parse of the entry is lossy: a raw
/// Elo of 4095 reads as 4000, a corrupt date as unknown).
#[derive(Default, Clone, Copy)]
struct SidePatch {
    date: Option<PartialDate>,
    event: Option<u32>,
    round: Option<u32>,
    white_elo: Option<u16>,
    black_elo: Option<u16>,
}

impl SidePatch {
    fn is_empty(&self) -> bool {
        self.date.is_none()
            && self.event.is_none()
            && self.round.is_none()
            && self.white_elo.is_none()
            && self.black_elo.is_none()
    }
}

/// Which side gets what, computed from the two entries.
struct Plan {
    changes: Vec<SharedTag>,
    a: SidePatch,
    b: SidePatch,
}

#[allow(clippy::too_many_lines)]
fn plan(
    number_a: u32,
    entry_a: &IndexEntry,
    number_b: u32,
    entry_b: &IndexEntry,
    name: &dyn Fn(NameKind, u32) -> Option<Vec<u8>>,
) -> Plan {
    let mut p = Plan {
        changes: Vec::new(),
        a: SidePatch::default(),
        b: SidePatch::default(),
    };
    let text = |kind: NameKind, id: u32| {
        name(kind, id)
            .map(|n| crate::si4::bytes_to_string(&n).into_owned())
            .unwrap_or_default()
    };

    // Date: one is a strict prefix of the other; the longer one wins.
    let (da, db) = (trimmed_date(entry_a), trimmed_date(entry_b));
    if da != db && db.starts_with(&da) {
        p.changes.push(SharedTag {
            game: number_a,
            field: SharedField::Date,
            old: da.clone(),
            new: db.clone(),
        });
        p.a.date = Some(entry_b.date);
    } else if da != db && da.starts_with(&db) {
        p.changes.push(SharedTag {
            game: number_b,
            field: SharedField::Date,
            old: db.clone(),
            new: da.clone(),
        });
        p.b.date = Some(entry_a.date);
    }

    // Event and round: an empty side takes the other's name (same ID).
    for (field, kind, id_a, id_b) in [
        (
            SharedField::Event,
            NameKind::Event,
            entry_a.event_id,
            entry_b.event_id,
        ),
        (
            SharedField::Round,
            NameKind::Round,
            entry_a.round_id,
            entry_b.round_id,
        ),
    ] {
        let fa = fill_of(name(kind, id_a).as_deref());
        let fb = fill_of(name(kind, id_b).as_deref());
        let apply = |p: &mut Plan, to_a: bool, id: u32| {
            let side = if to_a { &mut p.a } else { &mut p.b };
            if field == SharedField::Event {
                side.event = Some(id);
            } else {
                side.round = Some(id);
            }
        };
        if fa == Fill::Empty && fb == Fill::Known {
            p.changes.push(SharedTag {
                game: number_a,
                field,
                old: text(kind, id_a),
                new: text(kind, id_b),
            });
            apply(&mut p, true, id_b);
        } else if fb == Fill::Empty && fa == Fill::Known {
            p.changes.push(SharedTag {
                game: number_b,
                field,
                old: text(kind, id_b),
                new: text(kind, id_a),
            });
            apply(&mut p, false, id_a);
        }
    }

    // Ratings: 0 takes the other side's value (the rating type is kept).
    for (field, elo_a, elo_b) in [
        (SharedField::WhiteElo, entry_a.white_elo, entry_b.white_elo),
        (SharedField::BlackElo, entry_a.black_elo, entry_b.black_elo),
    ] {
        let white = field == SharedField::WhiteElo;
        if elo_a == 0 && elo_b != 0 {
            p.changes.push(SharedTag {
                game: number_a,
                field,
                old: elo_a.to_string(),
                new: elo_b.to_string(),
            });
            if white {
                p.a.white_elo = Some(elo_b);
            } else {
                p.a.black_elo = Some(elo_b);
            }
        } else if elo_b == 0 && elo_a != 0 {
            p.changes.push(SharedTag {
                game: number_b,
                field,
                old: elo_b.to_string(),
                new: elo_a.to_string(),
            });
            if white {
                p.b.white_elo = Some(elo_a);
            } else {
                p.b.black_elo = Some(elo_a);
            }
        }
    }
    p
}

/// What `share_tags` would change between two games, without writing
/// (`sc_game tags share check`). Pure: the entries and a name lookup are the
/// inputs. Equal numbers or `0` are the caller's business
/// ([`share_tags_preview`](crate::maintenance::share_tags_preview) checks them).
pub fn plan_share_tags(
    number_a: u32,
    entry_a: &IndexEntry,
    number_b: u32,
    entry_b: &IndexEntry,
    name: &dyn Fn(NameKind, u32) -> Option<Vec<u8>>,
) -> Vec<SharedTag> {
    plan(number_a, entry_a, number_b, entry_b, name).changes
}

/// Applies `side` on the bytes `old` and nothing else: the event and round
/// IDs keep the site bits of byte 14, the date replaces the low 20 bits of the
/// date word (the event date bits stay, as in `IndexEntry::SetDate`), and an
/// Elo replaces the low 12 bits of its word (the rating type stays, as in
/// `SetWhiteElo`).
fn patch(old: &[u8; ENTRY_LEN], side: &SidePatch) -> [u8; ENTRY_LEN] {
    let mut out = *old;
    let high = |id: u32, mask: u32| u8::try_from((id >> 16) & mask).unwrap_or(0);
    let low16 = |id: u32| u16::try_from(id & 0xFFFF).unwrap_or(0).to_be_bytes();
    if let Some(id) = side.event {
        if let Some(b) = out.get_mut(14) {
            *b = (*b & !0b1110_0000) | (high(id, 7) << 5);
        }
        if let Some(dst) = out.get_mut(15..17) {
            dst.copy_from_slice(&low16(id));
        }
    }
    if let Some(id) = side.round {
        if let Some(b) = out.get_mut(14) {
            *b = (*b & !0b11) | high(id, 3);
        }
        if let Some(dst) = out.get_mut(19..21) {
            dst.copy_from_slice(&low16(id));
        }
    }
    if let Some(date) = side.date
        && let Some(dst) = out.get_mut(25..29)
        && let Ok(word) = <[u8; 4]>::try_from(&*dst)
    {
        let merged = (u32::from_be_bytes(word) & 0xFFF0_0000) | (date.to_scid_u32() & 0x000F_FFFF);
        dst.copy_from_slice(&merged.to_be_bytes());
    }
    for (start, elo) in [(29, side.white_elo), (31, side.black_elo)] {
        if let Some(elo) = elo
            && let Some(dst) = out.get_mut(start..start + 2)
            && let Ok(word) = <[u8; 2]>::try_from(&*dst)
        {
            let merged = (u16::from_be_bytes(word) & 0xF000) | (elo & 0x0FFF);
            dst.copy_from_slice(&merged.to_be_bytes());
        }
    }
    out
}

impl Si4Writer {
    /// Shares tags between games `a` and `b` (`sc_game tags share update`) and
    /// returns the changes made. Does nothing (empty list, no error) when the
    /// numbers are equal, `0` or outside the database. Only the bytes of the
    /// fields that change are replaced (the rest of both entries is left as it
    /// is, even if it is not canonical), with one `fsync`.
    ///
    /// The change is **not atomic** across the two games: if a write fails
    /// after game `a` was written, `a` keeps its new tags and `b` its old
    /// ones. Each entry stays valid, and the writer is poisoned.
    ///
    /// # Errors
    ///
    /// An I/O error or a corrupt entry.
    pub fn share_tags(&mut self, a: u32, b: u32) -> Result<Vec<SharedTag>> {
        self.check_alive()?;
        let len = self.len();
        if a == 0 || b == 0 || a == b || a > len || b > len {
            return Ok(Vec::new());
        }
        let entry_a = self.entry_at(a)?;
        let entry_b = self.entry_at(b)?;
        let plan = {
            let names = &self.names;
            plan(a, &entry_a, b, &entry_b, &|kind, id| {
                names.get(kind, id).map(<[u8]>::to_vec)
            })
        };
        // Read both records before the first write: a failed read must not
        // leave game `a` written and the index unsynced.
        let old_a = self.read_entry(a)?;
        let old_b = self.read_entry(b)?;
        let mut wrote = false;
        let mut failure = None;
        for (number, old, side) in [(a, &old_a, &plan.a), (b, &old_b, &plan.b)] {
            if side.is_empty() {
                continue;
            }
            let new = patch(old, side);
            if new == *old {
                continue;
            }
            let path = &self.index_path;
            let written = self
                .index_file
                .seek(SeekFrom::Start(entry_position(number)))
                .and_then(|_| self.index_file.write_all(&new))
                .map_err(io_error(path));
            if let Err(error) = written {
                // The entry may be torn.
                self.poisoned = true;
                failure = Some(error);
                break;
            }
            wrote = true;
        }
        // Whatever was written is synced, even when the second write failed.
        if wrote {
            let synced = self.sync_index();
            if let Some(error) = failure {
                return Err(error);
            }
            synced?;
        } else if let Some(error) = failure {
            return Err(error);
        }
        Ok(plan.changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_of(kind: NameKind, id: u32) -> Option<Vec<u8>> {
        let table: &[&[u8]] = match kind {
            NameKind::Event | NameKind::Round => &[b"?", b"Aberto", b"1", b""],
            _ => &[],
        };
        table.get(id as usize).map(|n| n.to_vec())
    }

    #[test]
    fn event_and_round_flow_from_the_known_side() {
        let a = IndexEntry {
            event_id: 0,
            round_id: 2,
            ..IndexEntry::default()
        };
        let b = IndexEntry {
            event_id: 1,
            round_id: 3,
            ..IndexEntry::default()
        };
        let changes = plan_share_tags(1, &a, 2, &b, &name_of);
        assert_eq!(
            changes,
            [
                SharedTag {
                    game: 1,
                    field: SharedField::Event,
                    old: "?".into(),
                    new: "Aberto".into()
                },
                SharedTag {
                    game: 2,
                    field: SharedField::Round,
                    old: String::new(),
                    new: "1".into()
                },
            ]
        );
    }

    #[test]
    fn a_missing_name_neither_gives_nor_receives() {
        let a = IndexEntry {
            event_id: 99,
            ..IndexEntry::default()
        };
        let b = IndexEntry {
            event_id: 1,
            ..IndexEntry::default()
        };
        assert_eq!(
            plan_share_tags(1, &a, 2, &b, &name_of),
            Vec::<SharedTag>::new()
        );
    }

    #[test]
    fn patch_keeps_the_site_bits() {
        let mut old = [0u8; ENTRY_LEN];
        old[14] = 0b0001_1100;
        let side = SidePatch {
            event: Some(1),
            ..SidePatch::default()
        };
        let out = patch(&old, &side);
        assert_eq!(out[14] & 0b0001_1100, 0b0001_1100);
        assert_eq!(out[15..17], [0, 1]);
    }

    #[test]
    fn patch_touches_only_the_bytes_of_the_field() {
        let old: [u8; ENTRY_LEN] = std::array::from_fn(|i| u8::try_from(i).unwrap_or(0) | 0x81);
        let side = SidePatch {
            white_elo: Some(2500),
            ..SidePatch::default()
        };
        let out = patch(&old, &side);
        let changed: Vec<usize> = (0..ENTRY_LEN).filter(|&i| out[i] != old[i]).collect();
        assert_eq!(changed, [29, 30]);
        // The rating type (high nibble) stays.
        assert_eq!(out[29] >> 4, old[29] >> 4);
        assert_eq!(u16::from_be_bytes([out[29], out[30]]) & 0x0FFF, 2500);
    }
}
