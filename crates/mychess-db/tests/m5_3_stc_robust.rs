//! Robustness of the `.stc` reader (M5-3a, D1048 and D1057 of
//! `docs/specs/m5-3-cache-stc.md`): truncation at every position, corruption
//! of single bytes, out-of-range fields, huge files, and the lazy read.
#![allow(
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

#[path = "common/m5_3_support.rs"]
mod stc;

use mychess_core::Position;
use mychess_db::tree::stc::{MAX_STC_BYTES, StcCache, StcError, remove_stc};
use mychess_db::tree::{CachedTree, TreeCache};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use stc::{
    entry, file, header, node, positions, simple_entry, start_board, stc_file, tmp_base, tree_of,
    write_stc,
};

fn corrupt<T: std::fmt::Debug>(r: Result<T, StcError>) {
    assert!(matches!(r, Err(StcError::Corrupt { .. })), "got {r:?}");
}

/// A valid file with several entries (copied and packed filters, both
/// sides to move), produced by the cache itself.
fn valid_file() -> Vec<u8> {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let ps = positions(3);
    let mut a = tree_of(40, 5);
    a.filter.set(7, 9);
    cache.store(&ps[0], a);
    let mut b = tree_of(6, 3);
    b.nodes.push(b.nodes[0].clone());
    b.nodes[1].mv = None;
    cache.store(&ps[1], b);
    let mut c = tree_of(3000, 8);
    c.filter.fill(0);
    cache.store(&ps[2], c);
    cache.store(&Position::new(), tree_of(2, 1));
    cache.write(None).unwrap();
    std::fs::read(stc_file(&base)).unwrap()
}

/// Reads `bytes` as the cache file of a scratch base and reports the cache.
fn read(bytes: &[u8]) -> (StcCache, Result<(), StcError>) {
    let (dir, base) = tmp_base();
    write_stc(&base, bytes);
    let mut cache = StcCache::new(Some(&base));
    let r = cache.read();
    drop(dir);
    (cache, r)
}

// ------------------------------------------------------------ truncation

#[test]
fn the_valid_file_reads_back() {
    let bytes = valid_file();
    let (cache, r) = read(&bytes);
    r.unwrap();
    assert_eq!(cache.info().0, 4);
}

#[test]
fn truncating_the_file_at_every_position_discards_it_whole() {
    let bytes = valid_file();
    let (dir, base) = tmp_base();
    for n in 0..bytes.len() {
        write_stc(&base, &bytes[..n]);
        let mut cache = StcCache::new(Some(&base));
        let r = cache.read();
        assert!(
            matches!(r, Err(StcError::Corrupt { .. })),
            "prefix of {n} bytes: {r:?}"
        );
        assert_eq!(cache.info().0, 0, "prefix of {n} bytes left entries");
        assert!(cache.lookup(&Position::new(), 2).is_none());
    }
    drop(dir);
}

#[test]
fn flipping_any_single_byte_never_panics_and_an_error_leaves_the_cache_empty() {
    let bytes = valid_file();
    let (dir, base) = tmp_base();
    for i in 0..bytes.len() {
        for value in [0x00u8, 0xFF, bytes[i] ^ 0x80, bytes[i].wrapping_add(1)] {
            let mut damaged = bytes.clone();
            damaged[i] = value;
            write_stc(&base, &damaged);
            let mut cache = StcCache::new(Some(&base));
            match cache.read() {
                Ok(()) => assert!(cache.info().0 <= 4, "byte {i} = {value}"),
                Err(StcError::Corrupt { .. }) => {
                    assert_eq!(cache.info().0, 0, "byte {i} = {value}");
                }
                Err(other) => panic!("byte {i} = {value}: {other:?}"),
            }
        }
    }
    drop(dir);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn random_damage_to_a_valid_file_never_panics(
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6),
        cut in prop::option::of(any::<prop::sample::Index>()),
    ) {
        let mut bytes = valid_file();
        for (at, value) in &edits {
            let i = at.index(bytes.len());
            bytes[i] = *value;
        }
        if let Some(c) = cut {
            bytes.truncate(c.index(bytes.len()));
        }
        let (mut cache, r) = read(&bytes);
        match r {
            Ok(()) => {
                // Whatever was accepted is usable.
                drop(cache.lookup(&Position::new(), 2));
            }
            Err(StcError::Corrupt { .. }) => prop_assert_eq!(cache.info().0, 0),
            Err(other) => prop_assert!(false, "{other:?}"),
        }
    }

    #[test]
    fn random_bytes_after_a_valid_header_never_panic(
        tail in prop::collection::vec(any::<u8>(), 0..400),
        in_use in 0u32..6,
    ) {
        let bytes = file(header(100, in_use, 0, 0), &[tail]);
        let (cache, r) = read(&bytes);
        if r.is_err() {
            prop_assert_eq!(cache.info().0, 0);
        }
    }

    #[test]
    fn random_files_with_any_header_never_panic(
        tail in prop::collection::vec(any::<u8>(), 0..300),
        size in any::<u32>(),
        in_use in any::<u32>(),
    ) {
        let bytes = file(header(size, in_use, 1, 1), &[tail]);
        let (cache, r) = read(&bytes);
        if r.is_err() {
            prop_assert_eq!(cache.info().0, 0);
        }
    }
}

// ------------------------------------------------------------ specific fields

#[test]
fn wrong_magic_is_corrupt_and_leaves_the_cache_empty() {
    let mut bytes = file(header(100, 1, 5, 0), &[simple_entry(3, 5)]);
    bytes[0] = b'x';
    let (cache, r) = read(&bytes);
    corrupt(r);
    assert_eq!(cache.info().0, 0);
}

#[test]
fn an_empty_file_and_a_short_header_are_corrupt() {
    for n in [0usize, 1, 4, 21] {
        let (_cache, r) = read(&header(1, 0, 0, 0)[..n]);
        corrupt(r);
    }
}

fn nodes(n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|_| node(12, 28, 7, [0, 1, 0, 0], 1)).collect()
}

#[test]
fn sixty_moves_are_accepted_and_sixty_one_are_corrupt_even_with_the_bytes_present() {
    let ok = entry(0, &start_board(), 60, 60, &nodes(60), (1, 1, &[0, 2]));
    let (cache, r) = read(&file(header(10, 1, 60, 0), &[ok]));
    r.unwrap();
    assert_eq!(cache.info().0, 1);

    let bad = entry(0, &start_board(), 61, 61, &nodes(61), (1, 1, &[0, 2]));
    let (cache, r) = read(&file(header(10, 1, 61, 0), &[bad]));
    corrupt(r);
    assert_eq!(cache.info().0, 0);
}

#[test]
fn a_huge_move_count_is_rejected_before_anything_is_read_for_it() {
    for count in [61u32, 1000, u32::MAX] {
        let e = entry(0, &start_board(), count, 1, &[], (1, 1, &[0, 2]));
        let (_cache, r) = read(&file(header(10, 1, 1, 0), &[e]));
        corrupt(r);
    }
}

#[test]
fn a_square_value_above_14_or_a_side_above_1_is_corrupt() {
    for bad in [15u8, 16, 64, 255] {
        let mut board = start_board();
        board[27] = bad;
        let e = entry(0, &board, 0, 0, &[], (1, 0, &[0, 0]));
        let (_cache, r) = read(&file(header(10, 1, 0, 0), &[e]));
        corrupt(r);
    }
    let mut board = start_board();
    board[27] = 14; // the largest valid code
    let e = entry(0, &board, 0, 0, &[], (1, 0, &[0, 0]));
    read(&file(header(10, 1, 0, 0), &[e])).1.unwrap();

    for side in [2u8, 3, 255] {
        let e = entry(side, &start_board(), 0, 0, &[], (1, 0, &[0, 0]));
        let (_cache, r) = read(&file(header(10, 1, 0, 0), &[e]));
        corrupt(r);
    }
}

#[test]
fn more_entries_than_the_cache_size_is_corrupt() {
    let (_c, r) = read(&file(
        header(1, 2, 0, 0),
        &[simple_entry(3, 1), simple_entry(3, 1)],
    ));
    corrupt(r);
    // NumInUse equal to the size is fine.
    let mut second = simple_entry(3, 1);
    second[0] = 1;
    read(&file(header(2, 2, 0, 0), &[simple_entry(3, 1), second]))
        .1
        .unwrap();
}

#[test]
fn an_absurd_number_of_entries_does_not_allocate_or_hang() {
    for in_use in [100_001u32, 1 << 24, u32::MAX] {
        let (cache, r) = read(&header(100_000, in_use, 0, 0));
        corrupt(r);
        assert_eq!(cache.info(), (0, 1000));
    }
}

#[test]
fn a_cache_size_of_zero_is_corrupt_even_with_no_entries() {
    corrupt(read(&header(0, 0, 0, 0)).1);
}

#[test]
fn compressed_length_that_disagrees_with_the_data_is_corrupt() {
    let ok = [0u8, 2, 0, 0]; // copied filter of 3 games
    let good = entry(0, &start_board(), 0, 0, &[], (3, 1, &ok));
    read(&file(header(10, 1, 0, 0), &[good])).1.unwrap();

    // Longer than the file.
    let mut e = entry(0, &start_board(), 0, 0, &[], (3, 1, &ok));
    let at = e.len() - ok.len() - 4;
    e[at..at + 4].copy_from_slice(&1000u32.to_be_bytes());
    corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);

    // Shorter than the real data: the decoder sees a copy of 2, not 3.
    let mut e = entry(0, &start_board(), 0, 0, &[], (3, 1, &ok));
    e[at..at + 4].copy_from_slice(&3u32.to_be_bytes());
    corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);

    // u32::MAX.
    let mut e = entry(0, &start_board(), 0, 0, &[], (3, 1, &ok));
    e[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);

    // Zero length: no flag byte.
    let e = entry(0, &start_board(), 0, 0, &[], (3, 1, &[]));
    corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);
}

#[test]
fn the_decoded_filter_must_have_the_declared_size() {
    // Declared 5 games, data decodes to 3 (copy of 3).
    let e = entry(0, &start_board(), 0, 0, &[], (5, 1, &[0, 2, 0, 0]));
    corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);
    // Declared 3 games, packed run of 100.
    let e = entry(0, &start_board(), 0, 0, &[], (3, 0, &[1, 0x02, 0, 100]));
    corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);
}

#[test]
fn filter_size_above_the_maximum_of_games_or_count_above_size_is_corrupt() {
    for size in [16_777_215u32, 1 << 30, u32::MAX] {
        let e = entry(0, &start_board(), 0, 0, &[], (size, 0, &[0, 0]));
        corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);
    }
    let e = entry(0, &start_board(), 0, 0, &[], (3, 4, &[0, 2, 0, 0]));
    corrupt(read(&file(header(10, 1, 0, 0), &[e])).1);
    // count == size is allowed
    let e = entry(0, &start_board(), 0, 0, &[], (3, 3, &[0, 2, 2, 2]));
    read(&file(header(10, 1, 0, 0), &[e])).1.unwrap();
}

#[test]
fn a_bad_entry_after_good_ones_discards_the_whole_file() {
    let mut second = simple_entry(3, 1);
    second[0] = 9;
    let bytes = file(header(10, 2, 1, 0), &[simple_entry(3, 1), second]);
    let (mut cache, r) = read(&bytes);
    corrupt(r);
    assert_eq!(cache.info().0, 0);
    assert!(cache.lookup(&Position::new(), 3).is_none());
}

#[test]
fn a_failed_read_does_not_disturb_a_cache_that_already_has_entries() {
    // The file is bad but the cache is not empty: read() does nothing at all.
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 4)); // no file yet
    write_stc(&base, b"garbage garbage garbage garbage");
    assert!(cache.read().is_ok());
    assert_eq!(cache.info().0, 1);
    assert!(cache.take_load_error().is_none());
}

// ------------------------------------------------------------ memory limits

#[cfg(unix)]
#[test]
fn a_file_over_512_mib_is_too_large_and_exactly_512_mib_is_not() {
    // Sparse files: no disk or memory is used.
    let (_dir, base) = tmp_base();
    let path = stc_file(&base);
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(MAX_STC_BYTES + 1).unwrap();
    drop(f);
    let mut cache = StcCache::new(Some(&base));
    let r = cache.read();
    assert!(
        matches!(r, Err(StcError::TooLarge { len }) if len == MAX_STC_BYTES + 1),
        "{r:?}"
    );
    assert_eq!(cache.info().0, 0);

    // Lazy read: reported by take_load_error, not by the search.
    assert!(cache.lookup(&Position::new(), 1).is_none());
    assert!(matches!(
        cache.take_load_error(),
        Some(StcError::TooLarge { .. })
    ));

    // Exactly at the limit it is read (and found to be zeros: bad magic).
    let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    f.set_len(MAX_STC_BYTES).unwrap();
    drop(f);
    let mut cache = StcCache::new(Some(&base));
    corrupt(cache.read());
}

#[test]
fn the_limit_is_512_mib() {
    assert_eq!(MAX_STC_BYTES, 512 * 1024 * 1024);
}

// ------------------------------------------------------------ lazy read

#[test]
fn a_missing_file_is_not_an_error_of_the_lookup() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    assert!(cache.lookup(&Position::new(), 3).is_none());
    assert!(cache.take_load_error().is_none());
    // But the explicit read reports it.
    let mut other = StcCache::new(Some(&base));
    assert!(matches!(other.read(), Err(StcError::Io { .. })));
}

#[test]
fn an_invalid_file_is_reported_once_through_take_load_error_and_the_lookup_misses() {
    let (_dir, base) = tmp_base();
    write_stc(&base, b"definitely not a cache");
    let mut cache = StcCache::new(Some(&base));
    assert!(cache.lookup(&Position::new(), 3).is_none());
    assert!(matches!(
        cache.take_load_error(),
        Some(StcError::Corrupt { .. })
    ));
    assert!(cache.take_load_error().is_none(), "taken");
    assert_eq!(cache.info().0, 0);
}

#[test]
fn only_one_read_is_attempted_per_state_and_clear_rearms_it() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    assert!(cache.lookup(&Position::new(), 3).is_none()); // file missing
    write_stc(&base, &file(header(100, 1, 5, 0), &[simple_entry(3, 5)]));
    assert!(cache.lookup(&Position::new(), 3).is_none(), "one attempt");
    cache.resize(2000);
    assert!(cache.lookup(&Position::new(), 3).is_none(), "resize");
    assert_eq!(cache.info().1, 2000);
    cache.clear();
    assert!(cache.lookup(&Position::new(), 3).is_some(), "after clear");
}

#[test]
fn the_file_is_read_by_the_first_lookup_and_the_file_capacity_wins() {
    let (_dir, base) = tmp_base();
    write_stc(&base, &file(header(33, 1, 5, 0), &[simple_entry(3, 5)]));
    let mut cache = StcCache::new(Some(&base));
    assert_eq!(cache.info(), (0, 1000), "nothing read yet");
    let hit = cache.lookup(&Position::new(), 3).unwrap();
    assert_eq!(hit.nodes[0].total, 5);
    assert_eq!(cache.info(), (1, 33));
}

#[test]
fn the_file_is_not_read_when_the_cache_already_has_entries() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 4)); // no file yet
    let other = positions(1)[0].clone();
    let mut seed = StcCache::new(None);
    seed.store(&other, tree_of(3, 9));
    seed.write_to(&base, None).unwrap();
    assert_eq!(seed.info().0, 1);
    cache.read().unwrap();
    assert_eq!(cache.info(), (1, 1000));
    assert!(cache.lookup(&Position::new(), 3).is_some());
    assert!(cache.lookup(&other, 3).is_none(), "the file was not merged");
}

#[test]
fn a_cache_without_a_database_never_reads_or_writes() {
    let mut cache = StcCache::new(None);
    cache.read().unwrap();
    assert_eq!(cache.write(None).unwrap(), 0);
    assert!(cache.lookup(&Position::new(), 3).is_none());
    assert!(cache.take_load_error().is_none());
    cache.store(&Position::new(), tree_of(3, 4));
    assert!(cache.lookup(&Position::new(), 3).is_some());
    assert_eq!(cache.base_path(), None);
}

#[test]
fn removing_the_file_and_clearing_makes_the_next_lookup_miss() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 4));
    cache.write(None).unwrap();
    assert!(remove_stc(&base).unwrap());
    cache.clear();
    assert!(cache.lookup(&Position::new(), 3).is_none());
    assert!(cache.take_load_error().is_none());
}

#[allow(dead_code)]
fn _types(_: CachedTree) {}
