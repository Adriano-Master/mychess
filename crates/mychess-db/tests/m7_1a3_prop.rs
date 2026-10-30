//! M7-1a3: properties of `repair_database` (spec M7-1, rule 43).
#![allow(clippy::unwrap_used)]

#[path = "common/m7_1a2_support.rs"]
mod support;

use std::fs;

use mychess_core::{Game, GameResult};
use mychess_db::si4::{RepairOptions, Si4Database, repair_database};
use mychess_db::{CancelToken, NoProgress};
use proptest::prelude::*;
use support::{build_games, line, snapshot};

fn games(n: usize) -> Vec<Game> {
    (0..n)
        .map(|i| {
            let mut g = line(&["e2e4", "e7e5"]);
            let t = g.tags_mut();
            t.white = format!("W{}", i % 4);
            t.black = format!("B{}", i % 3);
            t.event = format!("E{}", i % 2);
            t.result = GameResult::Draw;
            g
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Repairing a healthy database changes neither the index nor the game
    /// file, and repairing again is the identity.
    #[test]
    fn repairing_a_healthy_database_is_the_identity(n in 0usize..12) {
        let dir = tempfile::tempdir().unwrap();
        let base = build_games(dir.path(), &games(n));
        let before = snapshot(&base);
        repair_database(&base, RepairOptions::default(), &mut NoProgress, &CancelToken::new()).unwrap();
        let after = snapshot(&base);
        prop_assert_eq!(&after[0], &before[0]);
        prop_assert_eq!(&after[2], &before[2]);
        repair_database(&base, RepairOptions::default(), &mut NoProgress, &CancelToken::new()).unwrap();
        prop_assert_eq!(snapshot(&base), after);
    }

    /// Whatever bytes of the index are damaged, the repair never panics. A
    /// success is consistent with its report, leaves the `.sg4` alone and
    /// opens; a failure leaves all three files as they were.
    #[test]
    fn a_damaged_index_never_panics_and_a_repaired_one_opens(
        n in 1usize..8,
        flips in proptest::collection::vec((0usize..600, any::<u8>()), 0..12),
        cut in proptest::option::of(0usize..400),
        drop_unreadable in any::<bool>(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let base = build_games(dir.path(), &games(n));
        let path = base.with_extension("si4");
        let mut data = fs::read(&path).unwrap();
        for (at, byte) in flips {
            if let Some(slot) = data.get_mut(at) {
                // Keep the magic and version: those are errors, tested apart.
                if at >= 10 {
                    *slot = byte;
                }
            }
        }
        if let Some(cut) = cut {
            let keep = data.len().saturating_sub(cut).max(182);
            data.truncate(keep);
        }
        fs::write(&path, &data).unwrap();
        check_repair(&base, drop_unreadable)?;
    }

    /// Damage to the name file: huge counters in the header (bytes 12-35),
    /// random bytes anywhere, a cut.
    #[test]
    fn a_damaged_name_file_never_panics_and_leaves_the_files_or_a_database(
        n in 1usize..6,
        counters in proptest::collection::vec((12usize..36, any::<u8>()), 1..8),
        flips in proptest::collection::vec((36usize..200, any::<u8>()), 0..6),
        cut in proptest::option::of(0usize..60),
        drop_unreadable in any::<bool>(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let base = build_games(dir.path(), &games(n));
        let path = base.with_extension("sn4");
        let mut data = fs::read(&path).unwrap();
        for (at, byte) in counters.into_iter().chain(flips) {
            if let Some(slot) = data.get_mut(at) {
                *slot = byte;
            }
        }
        if let Some(cut) = cut {
            data.truncate(data.len().saturating_sub(cut));
        }
        fs::write(&path, &data).unwrap();
        check_repair(&base, drop_unreadable)?;
    }

    /// An index of a few thousand random entries (high name IDs, random
    /// offsets and lengths) over a small name file.
    #[test]
    fn a_random_index_of_thousands_of_entries_is_repaired_or_refused(
        count in 1000usize..4000,
        seed in any::<u64>(),
        drop_unreadable in any::<bool>(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let base = build_games(dir.path(), &games(3));
        let path = base.with_extension("si4");
        let mut data = fs::read(&path).unwrap();
        data.truncate(182);
        // xorshift: deterministic bytes from the seed.
        let mut state = seed | 1;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..count {
            for _ in 0..47 {
                data.push(next().to_le_bytes()[0]);
            }
        }
        // Keep the header's game count honest about the file.
        let count24 = u32::try_from(count).unwrap().to_be_bytes();
        data[14..17].copy_from_slice(&count24[1..]);
        fs::write(&path, &data).unwrap();
        check_repair(&base, drop_unreadable)?;
    }
}

/// Repairs `base` and checks what must hold whatever the damage was.
fn check_repair(base: &std::path::Path, drop_unreadable: bool) -> Result<(), TestCaseError> {
    let before = snapshot(base);
    let old_names = mychess_db::si4::NameBase::parse(&before[1]);
    let result = repair_database(
        base,
        RepairOptions { drop_unreadable },
        &mut NoProgress,
        &CancelToken::new(),
    );
    match result {
        Err(_) => {
            prop_assert_eq!(snapshot(base), before, "an error changes nothing");
        }
        Ok(report) => {
            let after = snapshot(base);
            prop_assert_eq!(&after[2], &before[2], ".sg4 untouched");
            prop_assert_eq!(report.games_read, report.games + report.entries_dropped);
            if !drop_unreadable {
                prop_assert_eq!(report.entries_dropped, 0);
            }
            prop_assert!(report.entries_beyond_header <= report.games_read);
            if let Ok(old) = old_names {
                for (removed, kind) in report
                    .names_removed
                    .iter()
                    .zip(mychess_db::si4::NameKind::ALL)
                {
                    // Names added as `?` may offset the removals by one.
                    prop_assert!(*removed <= old.num_names(kind), "{kind:?}");
                }
            }
            let mut db = Si4Database::open(base);
            prop_assert!(db.is_ok(), "{:?}", db.as_ref().err());
            let db = db.as_mut().unwrap();
            prop_assert_eq!(db.len(), report.games);
            if drop_unreadable {
                for number in 1..=db.len().min(50) {
                    prop_assert!(db.game(number).is_ok(), "game {number}");
                }
            }
            // A second run finds nothing to fix.
            let again = repair_database(
                base,
                RepairOptions { drop_unreadable },
                &mut NoProgress,
                &CancelToken::new(),
            );
            prop_assert!(again.is_ok());
            let again = again.unwrap();
            prop_assert_eq!(again.games, report.games);
            prop_assert_eq!(again.ids_fixed, 0);
            prop_assert!(!again.header_fixed);
        }
    }
    Ok(())
}
