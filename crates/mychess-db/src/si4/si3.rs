// Derived from Scid vs PC 4.27, src/tkscid.cpp (sc_base_upgrade, 3457-3560)
// and tcl/file.tcl (::file::Upgrade). Copyright (c) 1999-2003 Shane Hudson.
// All rights reserved (notices of the Scid sources); Scid vs PC contributors.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Upgrading an `.si3` database to SI4 (`docs/specs/m3-2-escrita-si4.md`,
//! rules 45 to 47).
//!
//! Only the index changes: the 128-byte header becomes the 182-byte one
//! (version 400, the 54 bytes of custom flag descriptions zero) and each
//! 46-byte entry becomes 47 bytes by inserting a zero byte (`Length_High`)
//! after the sixth. The `.sn3` and `.sg3` have the same format as `.sn4` and
//! `.sg4` and are only renamed; then the `.si3` is removed, as the interface
//! of Scid does (`file.tcl:429-431`). All of it is one journaled transaction
//! (the new `.si4` is renamed last).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use super::bytes::{read_u16_be, read_u24_be};
use super::database::{find_file, strip_known_extension};
use super::error::{Result, Si4Error, Si4File};
use super::header::{ENTRY_LEN, HEADER_LEN, MAGIC, SUPPORTED_VERSION};
use super::journal::{self, FailPoint, run_transaction};
use super::write::{io_error, remove_cache};

/// Size of the `.si3` header.
pub const SI3_HEADER_LEN: usize = 128;
/// Size of an `.si3` entry.
pub const SI3_ENTRY_LEN: usize = 46;
/// The oldest `.si3` version accepted (Scid 3.x).
pub const MIN_SI3_VERSION: u16 = 300;

/// What [`upgrade`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpgradeReport {
    /// The number of games converted.
    pub games: u32,
}

/// Converts the bytes of an `.si3` into the bytes of an `.si4`.
///
/// Errors: fewer than 128 bytes, or fewer entry bytes than `numGames`
/// declares, is [`Si4Error::Truncated`]; another magic is
/// [`Si4Error::BadMagic`]; a version below 300 is [`Si4Error::OldVersion`]
/// and 400 or more is [`Si4Error::NewerVersion`]. Bytes after the last entry
/// are ignored.
pub fn convert_index(si3: &[u8]) -> Result<Vec<u8>> {
    let truncated = |offset: usize, need: usize| Si4Error::Truncated {
        file: Si4File::Index,
        offset: offset as u64,
        need: need as u64,
    };
    let header = si3
        .get(..SI3_HEADER_LEN)
        .ok_or_else(|| truncated(si3.len(), SI3_HEADER_LEN - si3.len()))?;
    if header.get(..MAGIC.len()) != Some(&MAGIC[..]) {
        return Err(Si4Error::BadMagic {
            file: Si4File::Index,
        });
    }
    let version = read_u16_be(header, 8).ok_or_else(|| truncated(8, 2))?;
    if version < MIN_SI3_VERSION {
        return Err(Si4Error::OldVersion { found: version });
    }
    if version >= SUPPORTED_VERSION {
        return Err(Si4Error::NewerVersion { found: version });
    }
    let games = read_u24_be(header, 14).ok_or_else(|| truncated(14, 3))?;
    let games_usize = usize::try_from(games).unwrap_or(usize::MAX);
    let need = games_usize
        .checked_mul(SI3_ENTRY_LEN)
        .and_then(|n| n.checked_add(SI3_HEADER_LEN))
        .ok_or_else(|| truncated(si3.len(), usize::MAX))?;
    if si3.len() < need {
        return Err(truncated(si3.len(), need - si3.len()));
    }
    let mut out = Vec::with_capacity(HEADER_LEN + games_usize * ENTRY_LEN);
    // Magic, then version 400, then baseType, numGames, autoLoad and the
    // description copied as they are (bytes 10..128).
    out.extend_from_slice(&header[..8]);
    out.extend_from_slice(&SUPPORTED_VERSION.to_be_bytes());
    out.extend_from_slice(&header[10..SI3_HEADER_LEN]);
    out.resize(HEADER_LEN, 0);
    for entry in si3[SI3_HEADER_LEN..need].as_chunks::<SI3_ENTRY_LEN>().0 {
        out.extend_from_slice(&entry[..6]);
        out.push(0);
        out.extend_from_slice(&entry[6..]);
    }
    Ok(out)
}

/// Upgrades the database `base` (`.si3`, `.sn3`, `.sg3`) to SI4.
///
/// Refuses with [`Si4Error::AlreadyExists`] if any of `.si4`, `.sn4`, `.sg4`
/// exists, and fails with the error of the missing file if `.sn3` or `.sg3`
/// is absent; nothing is written in these cases. Afterwards open the result
/// with [`Si4Writer::open`](super::Si4Writer::open) or
/// [`Si4Database::open`](super::Si4Database::open).
pub fn upgrade(base: &Path) -> Result<UpgradeReport> {
    upgrade_with_hook(base, &mut FailPoint::never())
}

/// [`upgrade`] with a [`FailPoint`] to simulate a crash at each stage.
pub fn upgrade_with_hook(base: &Path, hook: &mut FailPoint) -> Result<UpgradeReport> {
    let stem = strip_known_extension(base);
    let stem = match stem.extension().and_then(|e| e.to_str()) {
        Some(e)
            if ["si3", "sn3", "sg3"]
                .iter()
                .any(|k| e.eq_ignore_ascii_case(k)) =>
        {
            stem.with_extension("")
        }
        _ => stem,
    };
    journal::recover(&stem)?;
    let si3_path = find_file(&stem, "si3");
    let sn3_path = find_file(&stem, "sn3");
    let sg3_path = find_file(&stem, "sg3");
    let si4_path = find_file(&stem, "si4");
    let sn4_path = find_file(&stem, "sn4");
    let sg4_path = find_file(&stem, "sg4");
    // Everything is checked inside the transaction, which holds the
    // `<base>.lock` lock, so two processes cannot both pass the checks
    // (decision D321).
    let games = run_transaction(&stem, "upgrade-si3", hook, |txn| {
        for path in [&si4_path, &sn4_path, &sg4_path] {
            if path.exists() {
                return Err(Si4Error::AlreadyExists { path: path.clone() });
            }
        }
        let mut data = Vec::new();
        File::open(&si3_path)
            .and_then(|mut f| f.read_to_end(&mut data))
            .map_err(io_error(&si3_path))?;
        let converted = convert_index(&data)?;
        drop(data);
        for path in [&sn3_path, &sg3_path] {
            fs::metadata(path).map_err(io_error(path))?;
        }
        let games = read_u24_be(&converted, 14).unwrap_or(0);
        remove_cache(&stem);
        txn.stage_rename(&sg3_path, &sg4_path)?;
        txn.stage_rename(&sn3_path, &sn4_path)?;
        // The new index is renamed last: it is what makes the database exist.
        txn.stage_file(&si4_path, |out, tmp| {
            out.write_all(&converted).map_err(io_error(tmp))
        })?;
        txn.stage_remove(&si3_path)?;
        Ok(games)
    })?;
    Ok(UpgradeReport { games })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::si4::Si4Database;
    use crate::si4::names_write::NameBuilder;

    fn si3_header(version: u16, games: u32) -> Vec<u8> {
        let mut h = vec![0u8; SI3_HEADER_LEN];
        h[..8].copy_from_slice(b"Scid.si\0");
        h[8..10].copy_from_slice(&version.to_be_bytes());
        h[10..14].copy_from_slice(&7u32.to_be_bytes());
        h[14..17].copy_from_slice(&games.to_be_bytes()[1..]);
        h[17..20].copy_from_slice(&[0, 0, 5]);
        h[20..24].copy_from_slice(b"desc");
        h
    }

    fn si3_entry(seed: u8) -> Vec<u8> {
        (0..SI3_ENTRY_LEN)
            .map(|i| seed.wrapping_add(i as u8))
            .collect()
    }

    #[test]
    fn converts_header_and_entries() {
        let mut si3 = si3_header(300, 2);
        si3.extend(si3_entry(1));
        si3.extend(si3_entry(100));
        si3.extend_from_slice(b"junk");
        let si4 = convert_index(&si3).unwrap();
        assert_eq!(si4.len(), HEADER_LEN + 2 * ENTRY_LEN);
        assert_eq!(&si4[..8], b"Scid.si\0");
        assert_eq!(&si4[8..10], &400u16.to_be_bytes());
        assert_eq!(&si4[10..128], &si3[10..128]);
        assert!(si4[128..182].iter().all(|&b| b == 0));
        let e = &si4[182..182 + ENTRY_LEN];
        assert_eq!(&e[..6], &si3_entry(1)[..6]);
        assert_eq!(e[6], 0);
        assert_eq!(&e[7..], &si3_entry(1)[6..]);
    }

    #[test]
    fn rejects_bad_input() {
        let mut bad = si3_header(300, 0);
        bad[0] = b'X';
        assert!(matches!(
            convert_index(&bad),
            Err(Si4Error::BadMagic { .. })
        ));
        assert!(matches!(
            convert_index(&si3_header(200, 0)),
            Err(Si4Error::OldVersion { found: 200 })
        ));
        assert!(matches!(
            convert_index(&si3_header(400, 0)),
            Err(Si4Error::NewerVersion { .. })
        ));
        assert!(matches!(
            convert_index(&si3_header(300, 0)[..127]),
            Err(Si4Error::Truncated { .. })
        ));
        let mut short = si3_header(300, 2);
        short.extend(si3_entry(1));
        short.extend(&si3_entry(2)[..45]);
        assert!(matches!(
            convert_index(&short),
            Err(Si4Error::Truncated { .. })
        ));
    }

    fn setup(dir: &Path) {
        let mut si3 = si3_header(300, 0);
        si3[10..14].copy_from_slice(&1u32.to_be_bytes());
        fs::write(dir.join("old.si3"), &si3).unwrap();
        fs::write(dir.join("old.sn3"), NameBuilder::new().to_bytes()).unwrap();
        fs::write(dir.join("old.sg3"), b"").unwrap();
    }

    #[test]
    fn upgrade_renames_files_and_opens() {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path());
        let report = upgrade(&dir.path().join("old")).unwrap();
        assert_eq!(report.games, 0);
        assert!(!dir.path().join("old.si3").exists());
        assert!(dir.path().join("old.sn4").exists());
        assert!(dir.path().join("old.sg4").exists());
        let db = Si4Database::open(&dir.path().join("old")).unwrap();
        assert_eq!(db.header().base_type(), 1);
        // Upgrading again is refused (and there is no .si3 anyway).
        setup(dir.path());
        assert!(matches!(
            upgrade(&dir.path().join("old")),
            Err(Si4Error::AlreadyExists { .. })
        ));
    }

    #[test]
    fn every_crash_point_of_the_upgrade_recovers_to_old_or_new() {
        let mut probe = FailPoint::never();
        {
            let dir = tempfile::tempdir().unwrap();
            setup(dir.path());
            upgrade_with_hook(&dir.path().join("old"), &mut probe).unwrap();
        }
        let total = probe.reached().len();
        for i in 0..total {
            let dir = tempfile::tempdir().unwrap();
            setup(dir.path());
            let base = dir.path().join("old");
            let mut hook = FailPoint::at(i);
            assert!(upgrade_with_hook(&base, &mut hook).is_err());
            let committed = hook.reached().iter().any(|l| l == "journal");
            if committed {
                journal::recover(&base).unwrap();
                Si4Database::open(&base).unwrap();
                assert!(!dir.path().join("old.si3").exists(), "stage {i}");
            } else {
                // Still the old database; a retry completes the upgrade.
                assert!(dir.path().join("old.si3").exists(), "stage {i}");
                assert!(dir.path().join("old.sg3").exists(), "stage {i}");
                upgrade(&base).unwrap();
                Si4Database::open(&base).unwrap();
            }
            let junk = fs::read_dir(dir.path()).unwrap().flatten().any(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                matches!(
                    Path::new(&n).extension().and_then(|e| e.to_str()),
                    Some("tmp" | "txn")
                )
            });
            assert!(!junk, "stage {i}");
        }
    }
}
