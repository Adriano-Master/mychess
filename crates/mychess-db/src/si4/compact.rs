// Derived from Scid vs PC 4.27, src/tkscid.cpp (sc_compact games / names,
// 4206-4580). Copyright (c) 1999-2003 Shane Hudson. All rights reserved
// (notices of the Scid sources); Scid vs PC contributors. Scid is distributed
// under the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Compaction of games and of names (`docs/specs/m3-2-escrita-si4.md`, rules
//! 42 and 43).
//!
//! Both operations replace several files, so they run as a journaled
//! transaction ([`super::journal`]): new files as `.tmp`, the journal, then
//! the renames (`.sg4`, `.sn4`, `.si4` last). They consume the [`Si4Writer`]
//! (every handle and the lock are released before the renames, which matters
//! on Windows) and hand back a new one opened on the result. If an operation
//! fails, the writer is gone: open the database again, which also finishes a
//! committed journal.
//!
//! Records and entries are copied as raw bytes; only the fields that must
//! change are patched (offset, `NumHalfMoves`, name IDs), so everything
//! inherited from Scid is preserved byte for byte (decision D284).

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::entry::{GameFlags, IndexEntry};
use super::error::{Result, Si4Error};
use super::gfile::GameFile;
use super::header::{ENTRY_LEN, HEADER_LEN};
use super::journal::{FailPoint, run_transaction};
use super::names::NameKind;
use super::names_write::NameBuilder;
use super::raw::RawGame;
use super::write::{Si4Writer, entry_position, io_error, plan_append, remove_cache};

/// What [`compact_games`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactGamesReport {
    /// Games kept.
    pub kept: u32,
    /// Games dropped because of the `D` flag.
    pub deleted: u32,
    /// Games dropped because their record could not be read or decoded.
    pub corrupt: u32,
    /// Games whose `NumHalfMoves` was corrected.
    pub half_moves_fixed: u32,
    /// Size of the `.sg4` before.
    pub games_bytes_before: u64,
    /// Size of the `.sg4` after.
    pub games_bytes_after: u64,
}

/// What [`compact_names`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactNamesReport {
    /// Whether anything changed. `false` when no name had frequency 0.
    pub changed: bool,
    /// Names removed, per kind in the order of [`NameKind::ALL`].
    pub removed: [u32; 4],
}

/// Compacts the game file: drops the games with the `D` flag and the ones
/// that cannot be decoded, repacks the `.sg4` in index order with the block
/// rule of Scid, and rewrites the `.si4` (type, description, custom flag
/// descriptions and autoload are kept; the `.sn4` is not touched).
///
/// Consumes the writer and returns a new one on the compacted database.
pub fn compact_games(writer: Si4Writer) -> Result<(Si4Writer, CompactGamesReport)> {
    compact_games_with_hook(writer, &mut FailPoint::never())
}

/// [`compact_games`] with a [`FailPoint`] to simulate a crash at each stage.
pub fn compact_games_with_hook(
    writer: Si4Writer,
    hook: &mut FailPoint,
) -> Result<(Si4Writer, CompactGamesReport)> {
    writer.check_alive()?;
    let Si4Writer {
        index_path,
        games_path,
        stem,
        mut index_file,
        games_file,
        header_bytes,
        header,
        ..
    } = writer;
    let count = header.num_games();
    let reopen = stem.clone();
    let stem_copy = stem.clone();
    let report = run_transaction(&reopen, "compact-games", hook, move |txn| {
        remove_cache(&stem);
        let index = read_entries(&mut index_file, &index_path, count)?;
        let before = games_file.metadata().map_err(io_error(&games_path))?.len();
        let mut source = GameFile::new(&games_file, games_path.clone())?;
        let mut entries: Vec<u8> = Vec::with_capacity(index.len());
        let mut report = CompactGamesReport {
            kept: 0,
            deleted: 0,
            corrupt: 0,
            half_moves_fixed: 0,
            games_bytes_before: before,
            games_bytes_after: 0,
        };
        txn.stage_file(&games_path, |out, tmp| {
            let mut end = 0u64;
            let zeros = [0u8; 4096];
            for (n, raw_entry) in (1..=count).zip(index.as_chunks::<ENTRY_LEN>().0) {
                let Ok((entry, _)) = IndexEntry::parse_lenient(raw_entry.as_slice()) else {
                    report.corrupt += 1;
                    continue;
                };
                if entry.flags.contains(GameFlags::DELETE) {
                    report.deleted += 1;
                    continue;
                }
                let Ok(record) = source.read_record(n, entry.offset, entry.length) else {
                    report.corrupt += 1;
                    continue;
                };
                let Ok(game) = RawGame::parse(record) else {
                    report.corrupt += 1;
                    continue;
                };
                let half_moves = u16::try_from(game.mainline().len() % 1024).unwrap_or(0);
                let (offset, padding) = plan_append(end, record.len());
                let offset32 = u32::try_from(offset).map_err(|_| Si4Error::GamesFileFull)?;
                let mut left = padding;
                while left > 0 {
                    let step = usize::try_from(left)
                        .unwrap_or(zeros.len())
                        .min(zeros.len());
                    out.write_all(&zeros[..step]).map_err(io_error(tmp))?;
                    left -= u64::try_from(step).unwrap_or(left);
                }
                out.write_all(record).map_err(io_error(tmp))?;
                end = offset + u64::try_from(record.len()).unwrap_or(u64::MAX);

                let mut bytes = *raw_entry;
                bytes[..4].copy_from_slice(&offset32.to_be_bytes());
                if half_moves != entry.num_half_moves {
                    patch_half_moves(&mut bytes, half_moves);
                    report.half_moves_fixed += 1;
                }
                entries.extend_from_slice(&bytes);
                report.kept += 1;
            }
            report.games_bytes_after = end;
            Ok(())
        })?;
        let mut new_header = header_bytes.clone();
        new_header.set_num_games(report.kept);
        txn.stage_file(&index_path, |out, tmp| {
            out.write_all(new_header.as_bytes())
                .map_err(io_error(tmp))?;
            out.write_all(&entries).map_err(io_error(tmp))
        })?;
        // Handles (and the lock) are released when the closure returns.
        Ok(report)
    })?;
    Ok((Si4Writer::open(&stem_copy)?, report))
}

/// Compacts the name file: when some name has frequency 0 it is removed and
/// the IDs of the others are renumbered keeping their order; the entries of
/// the `.si4` are rewritten with the new IDs. `maxFrequency` becomes the real
/// maximum and `timeStamp` 0 (rules 30 and 31). Does nothing (no file is
/// touched) if every name is used.
///
/// Consumes the writer and returns a new one.
pub fn compact_names(writer: Si4Writer) -> Result<(Si4Writer, CompactNamesReport)> {
    compact_names_with_hook(writer, &mut FailPoint::never())
}

/// [`compact_names`] with a [`FailPoint`] to simulate a crash at each stage.
pub fn compact_names_with_hook(
    writer: Si4Writer,
    hook: &mut FailPoint,
) -> Result<(Si4Writer, CompactNamesReport)> {
    writer.check_alive()?;
    // Build the new name base and the ID maps before touching anything.
    let mut fresh = NameBuilder::new();
    let mut maps: [Vec<u32>; 4] = Default::default();
    let mut report = CompactNamesReport::default();
    for kind in NameKind::ALL {
        let map = &mut maps[kind.index()];
        for id in 0..writer.names.num_names(kind) {
            let frequency = writer.names.frequency(kind, id).unwrap_or(0);
            if frequency == 0 {
                report.removed[kind.index()] += 1;
                map.push(u32::MAX);
                continue;
            }
            let name = writer.names.get(kind, id).unwrap_or(&[]);
            let new_id = fresh.add_name(kind, name)?;
            fresh.inc_frequency(kind, new_id, i32::try_from(frequency).unwrap_or(i32::MAX));
            map.push(new_id);
        }
    }
    if report.removed == [0; 4] {
        return Ok((writer, report));
    }
    report.changed = true;

    let Si4Writer {
        index_path,
        names_path,
        stem,
        mut index_file,
        header_bytes,
        header,
        ..
    } = writer;
    let count = header.num_games();
    let reopen = stem.clone();
    let stem_copy = stem.clone();
    run_transaction(&reopen, "compact-names", hook, move |txn| {
        remove_cache(&stem);
        let mut index = read_entries(&mut index_file, &index_path, count)?;
        for (n, bytes) in (1..=count).zip(index.as_chunks_mut::<ENTRY_LEN>().0) {
            let (entry, _) = IndexEntry::parse_lenient(bytes.as_slice()).map_err(|e| {
                Si4Error::CorruptIndex {
                    game: n,
                    reason: e.reason(),
                }
            })?;
            let remap = |kind: NameKind, id: u32| {
                maps[kind.index()]
                    .get(usize::try_from(id).unwrap_or(usize::MAX))
                    .copied()
                    .filter(|&new| new != u32::MAX)
                    .ok_or(Si4Error::CorruptIndex {
                        game: n,
                        reason: "entry uses a name with frequency 0",
                    })
            };
            patch_name_ids(
                bytes.as_mut_slice(),
                [
                    remap(NameKind::Player, entry.white_id)?,
                    remap(NameKind::Player, entry.black_id)?,
                    remap(NameKind::Event, entry.event_id)?,
                    remap(NameKind::Site, entry.site_id)?,
                    remap(NameKind::Round, entry.round_id)?,
                ],
            );
        }
        let names_bytes = fresh.to_bytes();
        txn.stage_file(&names_path, |out, tmp| {
            out.write_all(&names_bytes).map_err(io_error(tmp))
        })?;
        txn.stage_file(&index_path, |out, tmp| {
            out.write_all(header_bytes.as_bytes())
                .map_err(io_error(tmp))?;
            out.write_all(&index).map_err(io_error(tmp))
        })?;
        drop(index_file);
        Ok(())
    })?;
    Ok((Si4Writer::open(&stem_copy)?, report))
}

/// Reads the `count` entries (47 bytes each) after the header.
pub(super) fn read_entries(file: &mut std::fs::File, path: &Path, count: u32) -> Result<Vec<u8>> {
    let size = u64::from(count) * ENTRY_LEN as u64;
    file.seek(SeekFrom::Start(entry_position(1)))
        .map_err(io_error(path))?;
    let mut out = Vec::new();
    file.take(size)
        .read_to_end(&mut out)
        .map_err(io_error(path))?;
    if (out.len() as u64) < size {
        return Err(Si4Error::Truncated {
            file: super::error::Si4File::Index,
            offset: HEADER_LEN as u64 + out.len() as u64,
            need: size - out.len() as u64,
        });
    }
    Ok(out)
}

/// Sets `NumHalfMoves` (10 bits: byte 37 and the top 2 bits of byte 38).
fn patch_half_moves(entry: &mut [u8; ENTRY_LEN], half_moves: u16) {
    let half_moves = half_moves & 0x3FF;
    entry[37] = u8::try_from(half_moves & 0xFF).unwrap_or(0);
    entry[38] = (entry[38] & 0x3F) | (u8::try_from(half_moves >> 8).unwrap_or(0) << 6);
}

/// Writes the five name IDs (White, Black, Event, Site, Round) into the
/// entry bytes, the layout of [`IndexEntry::to_bytes`]; bytes 9 and 14 hold
/// nothing but ID bits.
pub(super) fn patch_name_ids(entry: &mut [u8], ids: [u32; 5]) {
    let [white, black, event, site, round] = ids;
    let high = |v: u32, shift: u32, mask: u32| u8::try_from((v >> 16) & mask).unwrap_or(0) << shift;
    let low = |v: u32| u16::try_from(v & 0xFFFF).unwrap_or(0).to_be_bytes();
    let mut put = |at: usize, data: &[u8]| {
        if let Some(dst) = entry.get_mut(at..at + data.len()) {
            dst.copy_from_slice(data);
        }
    };
    put(9, &[high(white, 4, 0xF) | high(black, 0, 0xF)]);
    put(10, &low(white));
    put(12, &low(black));
    put(
        14,
        &[high(event, 5, 7) | high(site, 2, 7) | high(round, 0, 3)],
    );
    put(15, &low(event));
    put(17, &low(site));
    put(19, &low(round));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::si4::Si4Database;
    use mychess_core::{Game, GameResult};
    use std::fs;

    fn game(white: &str, black: &str, moves: &[&str]) -> Game {
        let mut g = Game::new();
        let mut node = g.root();
        for uci in moves {
            let mv = g.position_at(node).unwrap().parse_uci(uci).unwrap();
            node = g.add_move(node, mv).unwrap();
        }
        let tags = g.tags_mut();
        tags.white = white.into();
        tags.black = black.into();
        tags.event = "Test".into();
        tags.result = GameResult::Draw;
        g
    }

    fn sample_db(dir: &tempfile::TempDir) -> Si4Writer {
        let mut w = Si4Writer::create(&dir.path().join("db"), 1).unwrap();
        w.add_game(&game("A", "B", &["e2e4", "e7e5"]), GameFlags::empty())
            .unwrap();
        w.add_game(&game("C", "D", &["d2d4"]), GameFlags::empty())
            .unwrap();
        w.add_game(
            &game("A", "D", &["g1f3", "g8f6", "b1c3"]),
            GameFlags::empty(),
        )
        .unwrap();
        w
    }

    #[test]
    fn compact_games_drops_deleted_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = sample_db(&dir);
        w.set_flag(2, GameFlags::DELETE, super::super::write::FlagOp::Set)
            .unwrap();
        w.set_description("kept").unwrap();
        w.set_custom_flag_description(1, "mine").unwrap();
        let names_before = fs::read(w.names_path()).unwrap();
        let (w, report) = compact_games(w).unwrap();
        assert_eq!((report.kept, report.deleted, report.corrupt), (2, 1, 0));
        assert_eq!(w.len(), 2);
        assert_eq!(fs::read(w.names_path()).unwrap(), names_before);
        assert_eq!(w.header().description(), b"kept");
        drop(w);
        let mut db = Si4Database::open(&dir.path().join("db")).unwrap();
        assert_eq!(db.len(), 2);
        assert_eq!(db.game(1).unwrap().tags().white, "A");
        assert_eq!(db.game(2).unwrap().mainline().count(), 3);
        assert!(!dir.path().join("db.txn").exists());
    }

    #[test]
    fn compact_names_renumbers_and_drops_unused() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = sample_db(&dir);
        // Replacing game 2 leaves "C" with frequency 0.
        w.replace_game(2, &game("A", "B", &["d2d4"])).unwrap();
        assert_eq!(w.names().num_names(NameKind::Player), 4);
        let (w, report) = compact_names(w).unwrap();
        assert!(report.changed);
        assert_eq!(report.removed, [1, 0, 0, 0]);
        assert_eq!(w.names().num_names(NameKind::Player), 3);
        drop(w);
        let mut db = Si4Database::open(&dir.path().join("db")).unwrap();
        assert_eq!(db.game(2).unwrap().tags().white, "A");
        assert_eq!(db.game(3).unwrap().tags().black, "D");
        // A second run has nothing to do.
        let w = Si4Writer::open(&dir.path().join("db")).unwrap();
        let (_, again) = compact_names(w).unwrap();
        assert!(!again.changed);
    }

    #[test]
    fn name_id_patch_matches_entry_layout() {
        let entry = IndexEntry {
            white_id: 0xABCDE,
            black_id: 0x12345,
            event_id: 0x7_FFFF,
            site_id: 0x5_4321,
            round_id: 0x3_FFFF,
            ..IndexEntry::default()
        };
        let want = entry.to_bytes();
        let mut bytes = IndexEntry::default().to_bytes();
        patch_name_ids(&mut bytes, [0xABCDE, 0x12345, 0x7_FFFF, 0x5_4321, 0x3_FFFF]);
        assert_eq!(bytes, want);
    }

    #[test]
    fn name_id_patch_at_the_limits_round_trips_and_overwrites_the_high_bits() {
        // The widest ID of each field: 20, 19, 19 and 18 bits.
        let max = [0xF_FFFF, 0x7_FFFF, 0x7_FFFF, 0x3_FFFF];
        let cases = [
            [max[0], max[0], max[1], max[2], max[3]],
            [0, 0, 0, 0, 0],
            [0x1_0000, 0xF_0000, 0x1_0000, 0x4_0000, 0x1_0000],
            [0xF_FFFF, 0, 0x7_FFFF, 0, 0x3_FFFF],
            [0, 0xF_FFFF, 0, 0x7_FFFF, 0],
        ];
        for ids in cases {
            // Bytes 9 and 14 start full of ones: the patch must clear bits.
            let mut bytes = IndexEntry::default().to_bytes();
            bytes[9] = 0xFF;
            bytes[14] = 0xFF;
            patch_name_ids(&mut bytes, ids);
            let (entry, _) = IndexEntry::parse_lenient(&bytes).unwrap();
            assert_eq!(
                [
                    entry.white_id,
                    entry.black_id,
                    entry.event_id,
                    entry.site_id,
                    entry.round_id
                ],
                ids
            );
            let want = IndexEntry {
                white_id: ids[0],
                black_id: ids[1],
                event_id: ids[2],
                site_id: ids[3],
                round_id: ids[4],
                ..IndexEntry::default()
            };
            assert_eq!(bytes, want.to_bytes(), "{ids:?}");
        }
    }

    #[test]
    fn half_moves_patch_matches_entry_layout() {
        for n in [0u16, 255, 256, 1023] {
            let entry = IndexEntry {
                num_half_moves: n,
                ..IndexEntry::default()
            };
            let mut bytes = IndexEntry::default().to_bytes();
            patch_half_moves(&mut bytes, n);
            assert_eq!(bytes, entry.to_bytes(), "{n}");
        }
    }

    #[test]
    fn every_crash_point_of_compact_games_gives_old_or_new_database() {
        let mut probe = FailPoint::never();
        {
            let dir = tempfile::tempdir().unwrap();
            let mut w = sample_db(&dir);
            w.set_flag(1, GameFlags::DELETE, super::super::write::FlagOp::Set)
                .unwrap();
            compact_games_with_hook(w, &mut probe).unwrap();
        }
        let total = probe.reached().len();
        assert!(total >= 8);
        for i in 0..total {
            let dir = tempfile::tempdir().unwrap();
            let mut w = sample_db(&dir);
            w.set_flag(1, GameFlags::DELETE, super::super::write::FlagOp::Set)
                .unwrap();
            let mut hook = FailPoint::at(i);
            assert!(compact_games_with_hook(w, &mut hook).is_err());
            let committed = hook.reached().iter().any(|l| l == "journal");
            let base = dir.path().join("db");
            // The reader refuses a pending journal.
            if dir.path().join("db.txn").exists() {
                assert!(matches!(
                    Si4Database::open(&base),
                    Err(Si4Error::PendingJournal { .. })
                ));
            }
            let w = Si4Writer::open(&base).unwrap();
            assert_eq!(w.len(), if committed { 2 } else { 3 }, "stage {i}");
            drop(w);
            let mut db = Si4Database::open(&base).unwrap();
            for n in 1..=db.len() {
                db.game(n).unwrap();
            }
            let leftovers: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let ext = Path::new(&name).extension().and_then(|e| e.to_str());
                    matches!(ext, Some("tmp" | "txn"))
                })
                .collect();
            assert!(leftovers.is_empty(), "stage {i}: {leftovers:?}");
        }
    }

    #[test]
    fn compaction_refuses_a_poisoned_writer() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = sample_db(&dir);
        w.poisoned = true;
        assert!(matches!(compact_names(w), Err(Si4Error::WriterUnusable)));
        let mut w = Si4Writer::open(&dir.path().join("db")).unwrap();
        w.poisoned = true;
        assert!(matches!(compact_games(w), Err(Si4Error::WriterUnusable)));
    }
}
