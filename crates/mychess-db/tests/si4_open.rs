//! Contract tests of `Si4Database::open` on real files, entry verification
//! against the name base and `.sg4` blocks (spec M3-1, decisions D262, D263,
//! D272; `docs/formatos/si4.md` sections 4 and 8).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_lossless
)]

#[path = "common/si4_builder.rs"]
mod si4_builder;

use std::fs;
use std::io::Cursor;

use mychess_core::GameResult;
use mychess_db::si4::{BLOCK_SIZE, Si4Database, Si4Error, Si4File};
use pretty_assertions::assert_eq;
use rstest::rstest;
use si4_builder::{
    BLOCK, EntrySpec, IndexHeader, Parts, default_names, index_file, long_record, mem_db,
    names_file_with, open_parts, parts_from_records, rec, section, write_files,
};

/// 1.e4 e5 in the standard position (the example of the format document).
const E4_E5: [u8; 5] = [0x00, 0x00, 0xCF, 0xCF, 0x0F];

fn two_games() -> Parts {
    parts_from_records(&[E4_E5.to_vec(), rec(&[0xCF, 0xCF, 0xB1, 0x0F], &[])])
}

#[rstest]
#[case::lower(["si4", "sn4", "sg4"])]
#[case::upper(["SI4", "SN4", "SG4"])]
#[case::mixed(["Si4", "sN4", "sG4"])]
fn open_finds_the_files_regardless_of_extension_case(#[case] exts: [&str; 3]) {
    let dir = tempfile::tempdir().unwrap();
    let stem = write_files(dir.path(), "games", exts, &two_games());
    for variant in [
        stem.clone(),
        dir.path().join(format!("games.{}", exts[0])),
        dir.path().join(format!("games.{}", exts[1])),
        dir.path().join(format!("games.{}", exts[2])),
        dir.path().join("games.si4"),
        dir.path().join("games.SG4"),
    ] {
        let mut db = Si4Database::open(&variant).unwrap_or_else(|e| {
            panic!("{}: {e:?}", variant.display());
        });
        assert_eq!(db.len(), 2);
        assert_eq!(db.game(1).unwrap().mainline().count(), 2);
    }
}

#[test]
fn open_accepts_a_base_name_with_dots() {
    let dir = tempfile::tempdir().unwrap();
    let stem = write_files(
        dir.path(),
        "my.games.v2",
        ["si4", "sn4", "sg4"],
        &two_games(),
    );
    assert_eq!(Si4Database::open(&stem).unwrap().len(), 2);
    assert_eq!(
        Si4Database::open(&dir.path().join("my.games.v2.sn4"))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn open_reports_io_error_for_each_missing_file() {
    for missing in ["si4", "sn4", "sg4"] {
        let dir = tempfile::tempdir().unwrap();
        let stem = write_files(dir.path(), "g", ["si4", "sn4", "sg4"], &two_games());
        fs::remove_file(dir.path().join(format!("g.{missing}"))).unwrap();
        let e = Si4Database::open(&stem).unwrap_err();
        assert!(matches!(e, Si4Error::Io { .. }), "missing {missing}: {e:?}");
    }
}

#[test]
fn open_reports_the_error_of_the_file_that_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = two_games();
    p.index[0] = b'X';
    let stem = write_files(dir.path(), "a", ["si4", "sn4", "sg4"], &p);
    assert!(matches!(
        Si4Database::open(&stem),
        Err(Si4Error::BadMagic {
            file: Si4File::Index
        })
    ));
    let mut p = two_games();
    p.names[0] = b'X';
    let stem = write_files(dir.path(), "b", ["si4", "sn4", "sg4"], &p);
    assert!(matches!(
        Si4Database::open(&stem),
        Err(Si4Error::BadMagic {
            file: Si4File::Names
        })
    ));
    let mut p = two_games();
    p.index[8..10].copy_from_slice(&399u16.to_be_bytes());
    let stem = write_files(dir.path(), "c", ["si4", "sn4", "sg4"], &p);
    assert!(matches!(
        Si4Database::open(&stem),
        Err(Si4Error::OldVersion { found: 399 })
    ));
    let mut p = two_games();
    p.index.truncate(p.index.len() - 1);
    let stem = write_files(dir.path(), "d", ["si4", "sn4", "sg4"], &p);
    assert!(matches!(
        Si4Database::open(&stem),
        Err(Si4Error::Truncated {
            file: Si4File::Index,
            ..
        })
    ));
}

#[test]
fn open_does_not_need_write_access_and_never_modifies_the_files() {
    let dir = tempfile::tempdir().unwrap();
    let stem = write_files(dir.path(), "ro", ["si4", "sn4", "sg4"], &two_games());
    let paths: Vec<_> = ["si4", "sn4", "sg4"]
        .iter()
        .map(|e| dir.path().join(format!("ro.{e}")))
        .collect();
    let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    for p in &paths {
        let mut perms = fs::metadata(p).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(p, perms).unwrap();
    }
    {
        let mut db = Si4Database::open(&stem).unwrap();
        for n in 1..=db.len() {
            db.entry(n).unwrap();
            db.game(n).unwrap();
            db.decode_mainline(n).unwrap();
            db.raw_game(n).unwrap();
            db.record_bytes(n).unwrap();
        }
    }
    let after: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    assert_eq!(before, after);
    for p in &paths {
        let mut perms = fs::metadata(p).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(p, perms).unwrap();
    }
}

#[test]
fn open_reads_the_example_of_the_format_document() {
    let mut db = mem_db(&[E4_E5.to_vec()]);
    let mv = db.decode_mainline(1).unwrap();
    assert_eq!(mv.len(), 2);
    assert_eq!(
        (mv[0].from.to_string(), mv[0].to.to_string()),
        ("e2".to_owned(), "e4".to_owned())
    );
    assert_eq!(
        (mv[1].from.to_string(), mv[1].to.to_string()),
        ("e7".to_owned(), "e5".to_owned())
    );
    assert!(mv.iter().all(|m| m.promotion.is_none()));
    let game = db.game(1).unwrap();
    assert_eq!(
        si4_builder::final_fen(&game),
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"
    );
}

#[test]
fn game_numbers_outside_one_to_len_are_invalid() {
    let mut db = mem_db(&[E4_E5.to_vec(), E4_E5.to_vec()]);
    for n in [0, 3, u32::MAX] {
        assert!(
            matches!(db.game(n), Err(Si4Error::InvalidGameNumber { number, len: 2 }) if number == n)
        );
        assert!(matches!(
            db.decode_mainline(n),
            Err(Si4Error::InvalidGameNumber { .. })
        ));
        assert!(matches!(
            db.raw_game(n),
            Err(Si4Error::InvalidGameNumber { .. })
        ));
        assert!(matches!(
            db.record_bytes(n),
            Err(Si4Error::InvalidGameNumber { .. })
        ));
        assert!(matches!(
            db.entry(n),
            Err(Si4Error::InvalidGameNumber { .. })
        ));
    }
}

// ------------------------------------------------- entries against the names

fn players_only_names(players: usize, events: usize, sites: usize, rounds: usize) -> Vec<u8> {
    let make =
        |n: usize, p: &str| -> Vec<String> { (0..n).map(|i| format!("{p}{i:03}")).collect() };
    let s = |v: &[String]| section(&v.iter().map(String::as_str).collect::<Vec<_>>());
    names_file_with(
        &[
            s(&make(players, "p")),
            s(&make(events, "e")),
            s(&make(sites, "s")),
            s(&make(rounds, "r")),
        ],
        [1; 4],
        None,
    )
}

fn open_with_entries(names: &[u8], entries: &[EntrySpec]) -> Result<si4_builder::MemDb, Si4Error> {
    let encoded: Vec<_> = entries.iter().map(EntrySpec::encode).collect();
    Si4Database::from_parts(
        index_file(&IndexHeader::default(), &encoded),
        names,
        Cursor::new(E4_E5.to_vec()),
        "x.sg4".into(),
    )
}

#[rstest]
#[case::white("white")]
#[case::black("black")]
#[case::event("event")]
#[case::site("site")]
#[case::round("round")]
fn name_ids_equal_to_the_name_count_are_corrupt_and_one_less_is_fine(#[case] which: &str) {
    let names = players_only_names(3, 4, 5, 6);
    let limit = |w: &str| match w {
        "white" | "black" => 3,
        "event" => 4,
        "site" => 5,
        _ => 6,
    };
    let set = |e: &mut EntrySpec, v: u32| match which {
        "white" => e.white = v,
        "black" => e.black = v,
        "event" => e.event = v,
        "site" => e.site = v,
        _ => e.round = v,
    };
    let mut ok = EntrySpec::new(0, 5);
    set(&mut ok, limit(which) - 1);
    assert!(
        open_with_entries(&names, &[ok]).is_ok(),
        "{which}: last id valid"
    );
    for bad_id in [limit(which), limit(which) + 1, 0xF_FFFF] {
        let mut bad = EntrySpec::new(0, 5);
        set(&mut bad, bad_id);
        let e = open_with_entries(&names, &[EntrySpec::new(0, 5), EntrySpec::new(0, 5), bad])
            .unwrap_err();
        assert!(
            matches!(e, Si4Error::CorruptIndex { game: 3, .. }),
            "{which} id {bad_id}: {e:?}"
        );
    }
}

#[test]
fn open_reports_the_first_corrupt_game_when_several_are_corrupt() {
    let names = players_only_names(2, 1, 1, 1);
    let mut g2 = EntrySpec::new(0, 5);
    g2.event = 9;
    let mut g3 = EntrySpec::new(0, 5);
    g3.white = 7;
    let mut g4 = EntrySpec::new(0, 5);
    g4.result = 12;
    let e = open_with_entries(&names, &[EntrySpec::new(0, 5), g2, g3, g4]).unwrap_err();
    assert!(matches!(e, Si4Error::CorruptIndex { game: 2, .. }), "{e:?}");
}

#[test]
fn an_empty_name_base_makes_every_game_corrupt() {
    let names = players_only_names(0, 0, 0, 0);
    let e = open_with_entries(&names, &[EntrySpec::new(0, 5)]).unwrap_err();
    assert!(matches!(e, Si4Error::CorruptIndex { game: 1, .. }), "{e:?}");
}

// ------------------------------------------------------------- .sg4 blocks

/// An `.sg4` of `size` zero bytes with `E4_E5` copied at each offset given.
fn sg4_with(size: usize, at: &[usize]) -> Vec<u8> {
    let mut sg4 = vec![0u8; size];
    for &o in at {
        sg4[o..o + E4_E5.len()].copy_from_slice(&E4_E5);
    }
    sg4
}

fn db_with(entries: &[EntrySpec], sg4: Vec<u8>) -> si4_builder::MemDb {
    let encoded: Vec<_> = entries.iter().map(EntrySpec::encode).collect();
    let p = Parts {
        index: index_file(&IndexHeader::default(), &encoded),
        names: default_names(),
        sg4,
    };
    open_parts(&p).unwrap()
}

#[test]
fn block_size_is_131072() {
    assert_eq!(BLOCK_SIZE, 131_072);
    assert_eq!(BLOCK as u64, BLOCK_SIZE);
}

#[test]
fn record_ending_exactly_at_a_block_end_and_record_at_a_block_start_are_valid() {
    let n = E4_E5.len();
    let mut db = db_with(
        &[
            EntrySpec::new((BLOCK - n) as u32, n as u32),
            EntrySpec::new(BLOCK as u32, n as u32),
            EntrySpec::new(0, n as u32),
        ],
        sg4_with(2 * BLOCK, &[BLOCK - n, BLOCK, 0]),
    );
    for g in 1..=3 {
        assert_eq!(db.game(g).unwrap().mainline().count(), 2, "game {g}");
        assert_eq!(db.record_bytes(g).unwrap(), &E4_E5);
    }
}

#[test]
fn record_crossing_a_block_boundary_is_corrupt_with_the_game_number() {
    let n = E4_E5.len();
    for back in 1..n {
        let mut db = db_with(
            &[
                EntrySpec::new(0, n as u32),
                EntrySpec::new((BLOCK - back) as u32, n as u32),
            ],
            sg4_with(2 * BLOCK, &[0, BLOCK - back]),
        );
        assert!(db.game(1).is_ok());
        let e = db.game(2).unwrap_err();
        assert!(
            matches!(e, Si4Error::CorruptRecord { game: 2, .. }),
            "{back} bytes before the boundary: {e:?}"
        );
        assert!(db.decode_mainline(2).is_err());
        assert!(db.raw_game(2).is_err());
        assert!(db.record_bytes(2).is_err());
    }
}

#[test]
fn record_in_a_block_beyond_the_end_of_the_file_is_an_error() {
    let n = E4_E5.len() as u32;
    let mut db = db_with(
        &[EntrySpec::new(3 * BLOCK as u32, n)],
        sg4_with(2 * BLOCK, &[0]),
    );
    assert!(matches!(
        db.game(1),
        Err(Si4Error::CorruptRecord { game: 1, .. })
    ));
    // The block after the last one, right at its start.
    let mut db = db_with(
        &[EntrySpec::new(2 * BLOCK as u32, n)],
        sg4_with(2 * BLOCK, &[0]),
    );
    assert!(db.game(1).is_err());
}

#[test]
fn record_running_past_the_end_of_the_file_is_truncated() {
    let n = E4_E5.len();
    // The file has a partial second block of 3 bytes; the record needs 5.
    let mut sg4 = sg4_with(BLOCK, &[0]);
    sg4.extend_from_slice(&E4_E5[..3]);
    let mut db = db_with(&[EntrySpec::new(BLOCK as u32, n as u32)], sg4);
    assert!(matches!(
        db.game(1),
        Err(Si4Error::Truncated {
            file: Si4File::Games,
            ..
        })
    ));
    // Last record one byte short of its declared length.
    let mut db = db_with(&[EntrySpec::new(0, n as u32 + 1)], E4_E5.to_vec());
    assert!(matches!(db.game(1), Err(Si4Error::Truncated { .. })));
    // A length that would need the whole 17 bits against a tiny file.
    let mut db = db_with(&[EntrySpec::new(0, 131_071)], E4_E5.to_vec());
    assert!(matches!(db.game(1), Err(Si4Error::Truncated { .. })));
}

#[test]
fn empty_sg4_file_gives_errors_for_any_record() {
    let mut db = db_with(&[EntrySpec::new(0, 5)], Vec::new());
    assert!(db.game(1).is_err());
    assert_eq!(db.games_file_len(), 0);
}

#[test]
fn zero_length_record_is_corrupt() {
    let mut db = db_with(&[EntrySpec::new(0, 0)], sg4_with(64, &[0]));
    assert!(matches!(
        db.game(1),
        Err(Si4Error::CorruptRecord { game: 1, .. })
    ));
}

#[test]
fn offsets_near_u32_max_do_not_overflow() {
    for offset in [u32::MAX, u32::MAX - 4, u32::MAX - 131_071, 0xFFFF_0000] {
        let mut db = db_with(
            &[EntrySpec::new(offset, 131_071), EntrySpec::new(offset, 1)],
            sg4_with(64, &[0]),
        );
        assert!(db.game(1).is_err(), "offset {offset}");
        assert!(db.game(2).is_err(), "offset {offset}");
        assert!(db.record_bytes(1).is_err());
    }
}

#[test]
fn record_of_131071_bytes_is_readable_with_the_17th_length_bit() {
    let rec = long_record(131_071);
    let mut sg4 = rec.clone();
    sg4.resize(BLOCK, 0);
    let mut db = db_with(&[EntrySpec::new(0, 131_071)], sg4);
    assert_eq!(db.entry(1).unwrap().length, 131_071);
    assert_eq!(db.record_bytes(1).unwrap().len(), 131_071);
    let moves = db.decode_mainline(1).unwrap();
    assert_eq!(moves.len(), 131_068);
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.mainline().len(), 131_068);
}

#[test]
fn a_game_reading_one_record_does_not_break_the_next_reads() {
    let mut db = db_with(
        &[
            EntrySpec::new(0, 5),
            EntrySpec::new(5, 4), // bytes of the record shifted: not a valid game
            EntrySpec::new(0, 5),
        ],
        sg4_with(64, &[0]),
    );
    assert!(db.game(1).is_ok());
    assert!(db.game(2).is_err());
    assert!(db.game(3).is_ok());
    let mut results = Vec::new();
    for n in 1..=3 {
        results.push(db.entry(n).unwrap().result);
    }
    assert_eq!(results, vec![GameResult::Unknown; 3]);
}
