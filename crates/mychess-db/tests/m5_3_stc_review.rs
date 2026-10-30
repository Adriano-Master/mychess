//! Regression tests for the corrections of the M5-3a review (D1062 to D1065 of
//! `docs/specs/m5-3-cache-stc.md`, `docs/formatos/stc.md`): the exclusive
//! temporary file, the en passant SAN, the 32-bit sums, the stale entry and
//! the counts of the priming report.
#![allow(
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m5_3_support.rs"]
mod stc;
#[path = "common/m5_1_support.rs"]
mod support;

use std::path::Path;

use mychess_core::{Color, Position, Square};
use mychess_db::si4::RawMove;
use mychess_db::tree::stc::format::CompressedFilter;
use mychess_db::tree::stc::format::{
    HEADER_LEN, NODE_LEN, NodeExtras, StcEntry, StcHeader, encode_node,
};
use mychess_db::tree::stc::{
    MAX_STC_BYTES, MAX_WRITE_BYTES, StcCache, StcError, prime_with_lines, stc_path,
};
use mychess_db::tree::{CachedTree, TreeCache, TreeOptions, TreeState};
use mychess_db::{CancelToken, Filter, NoProgress};
use pretty_assertions::assert_eq;
use rstest::rstest;
use stc::{positions, start_board, stc_file, tmp_base, tree_of};
use support::{db_of, gb, pos_after, run, run_with, sans};

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn leftovers(dir: &Path) -> Vec<String> {
    names(dir)
        .into_iter()
        .filter(|n| n.contains(".tmp"))
        .collect()
}

// ------------------------------------------------------------ D1062: temporary file

#[test]
fn the_write_cap_constants_are_half_of_the_read_limit() {
    // D1062: the writer never produces a file the reader refuses.
    assert_eq!(MAX_WRITE_BYTES, 256 * 1024 * 1024);
    const { assert!(MAX_WRITE_BYTES <= MAX_STC_BYTES / 2) };
}

#[test]
fn no_temporary_is_left_after_a_success_nor_after_a_failure() {
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    cache.write(None).unwrap();
    assert_eq!(leftovers(dir.path()), Vec::<String>::new());

    let nowhere = dir.path().join("missing").join("games");
    assert!(cache.write_to(&nowhere, None).is_err());
    assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    assert_eq!(names(dir.path()), ["games.stc"]);
}

#[test]
fn two_writes_in_a_row_leave_a_readable_file() {
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    for p in &positions(3) {
        cache.store(p, tree_of(3, 2));
    }
    let a = cache.write(None).unwrap();
    let b = cache.write(None).unwrap();
    assert_eq!(a, b);
    assert_eq!(names(dir.path()), ["games.stc"]);
    let mut fresh = StcCache::new(Some(&base));
    fresh.read().unwrap();
    assert_eq!(fresh.info().0, 3);
}

#[test]
fn two_threads_writing_the_same_base_never_corrupt_the_file() {
    let (dir, base) = tmp_base();
    let probes = positions(6);
    let make = || {
        let mut cache = StcCache::new(Some(&base));
        for p in &probes {
            cache.store(p, tree_of(3, 2));
        }
        cache
    };
    let (a, b) = (make(), make());
    std::thread::scope(|s| {
        for cache in [&a, &b] {
            s.spawn(move || {
                for _ in 0..25 {
                    cache.write(None).expect("every write succeeds");
                }
            });
        }
    });
    assert_eq!(names(dir.path()), ["games.stc"], "no temporary left");
    let mut fresh = StcCache::new(Some(&base));
    fresh.read().expect("the file is whole");
    assert_eq!(fresh.info().0, 6);
    assert_eq!(
        std::fs::metadata(stc_file(&base)).unwrap().len(),
        a.write(None).unwrap()
    );
}

#[rstest]
#[case::ascii("a".repeat(240))]
#[case::two_byte_letters("\u{e9}".repeat(120))]
fn a_long_base_name_does_not_overflow_the_temporary_name(#[case] stem: String) {
    // The cache file name is 244 bytes (legal); the temporary is `.`+name+
    // `.pid.n.tmp`, which would pass 255 bytes unless the name is shortened.
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join(&stem);
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    let size = cache.write(None).unwrap();
    assert_eq!(std::fs::metadata(stc_path(&base)).unwrap().len(), size);
    assert_eq!(names(dir.path()).len(), 1, "{:?}", names(dir.path()));
}

#[cfg(unix)]
#[test]
fn the_permissions_of_an_existing_cache_file_are_kept() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, base) = tmp_base();
    std::fs::write(stc_file(&base), b"old").unwrap();
    std::fs::set_permissions(stc_file(&base), std::fs::Permissions::from_mode(0o640)).unwrap();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    cache.write(None).unwrap();
    let mode = std::fs::metadata(stc_file(&base))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640);
}

#[cfg(unix)]
#[test]
fn a_symlink_at_the_destination_is_kept_and_its_target_updated() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let real_dir = dir.path().join("real");
    std::fs::create_dir(&real_dir).unwrap();
    let real = real_dir.join("data.stc");
    std::fs::write(&real, b"old").unwrap();
    let base = dir.path().join("games");
    symlink(&real, stc_file(&base)).unwrap();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    let size = cache.write(None).unwrap();
    assert!(
        std::fs::symlink_metadata(stc_file(&base))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::metadata(&real).unwrap().len(), size);
    assert_eq!(
        names(&real_dir),
        ["data.stc"],
        "temporary next to the target"
    );
    assert_eq!(leftovers(dir.path()), Vec::<String>::new());
}

/// An incompressible filter of `len` games (values 1 to 255, pseudo-random).
fn noise_filter(len: u32, seed: u32) -> Filter {
    let mut f = Filter::new(len);
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(12345);
    for g in 0..len {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f.set(g, ((x >> 24) as u8).max(1));
    }
    f
}

/// D1062 cap. `write_capped` is private, so the real 256 MiB cap is
/// exercised: 18 entries of about 15 MiB do not fit, 17 do.
#[test]
#[ignore = "slow and big: builds and writes about 256 MiB (needs ~1 GiB of memory)"]
fn the_size_cap_keeps_the_biggest_totals_and_the_file_stays_readable() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let probes = positions(18);
    for (i, p) in probes.iter().enumerate() {
        let mut t = tree_of(15_000_000, 0);
        t.filter = noise_filter(15_000_000, i as u32);
        let total = i as u32 + 1;
        t.nodes[0].total = total;
        t.nodes[0].freq = [total, 0, 0, 0];
        cache.store(p, t);
    }
    let size = cache.write(None).unwrap();
    assert!(size <= MAX_WRITE_BYTES, "{size}");
    assert_eq!(cache.last_write_skipped(), 1);
    let bytes = std::fs::read(stc_file(&base)).unwrap();
    let h = StcHeader::from_bytes(&bytes[..HEADER_LEN]).unwrap();
    assert_eq!(h.num_in_use, 17);
    assert_eq!((h.lowest_total, h.lowest_total_index), (2, 0));
    drop(bytes);
    let mut fresh = StcCache::new(Some(&base));
    fresh.read().expect("not refused");
    assert_eq!(fresh.info().0, 17);
    assert_eq!(fresh.main_lowest(), Some((2, 0)));
    assert!(
        fresh.lookup(&probes[0], 15_000_000).is_none(),
        "lowest left out"
    );
    assert!(fresh.lookup(&probes[17], 15_000_000).is_some());
}

// ------------------------------------------------------------ D1063: en passant SAN

/// Offset of the first node of the first entry of a file.
const FIRST_NODE: usize = HEADER_LEN + 1 + 64 + 4 + 4;

fn one_move_tree(uci: &str) -> CachedTree {
    let sq = |text: &str| text.parse::<Square>().unwrap();
    let mut t = tree_of(3, 1);
    t.nodes[0].mv = Some(RawMove {
        from: sq(&uci[0..2]),
        to: sq(&uci[2..4]),
        promotion: None,
    });
    t
}

fn written_node(pos: &Position, uci: &str) -> Vec<u8> {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(pos, one_move_tree(uci));
    cache.write(None).unwrap();
    let bytes = std::fs::read(stc_file(&base)).unwrap();
    bytes[FIRST_NODE..FIRST_NODE + NODE_LEN].to_vec()
}

fn san_of(node: &[u8]) -> &[u8] {
    &node[13..21]
}

#[test]
fn an_en_passant_capture_is_written_as_exd6_with_the_ep_square() {
    // 1.e4 a6 2.e5 d5: White may play exd6 e.p.
    let pos = pos_after("e2e4 a7a6 e4e5 d7d5");
    let n = written_node(&pos, "e5d6");
    assert_eq!(san_of(&n), b"exd6\0\0\0\0");
    assert_eq!(n[10], 43, "epSquare = destination d6 (5 * 8 + 3)");
    assert_eq!(n[8], 35, "capturedSquare = d5, not the destination");
    assert_eq!((n[2], n[3]), (36, 43), "from e5, to d6");
}

#[test]
fn a_black_en_passant_capture_is_written_with_its_san() {
    // 1.a3 d5 2.a4 d4 3.e4 dxe3 e.p.
    let pos = pos_after("a2a3 d7d5 a3a4 d5d4 e2e4");
    let n = written_node(&pos, "d4e3");
    assert_eq!(san_of(&n), b"dxe3\0\0\0\0");
    assert_eq!(n[10], 20, "epSquare = e3 (2 * 8 + 4)");
    assert_eq!(n[8], 28, "capturedSquare = e4");
}

#[rstest]
#[case::push("", "e2e4", b"e4\0\0\0\0\0\0")]
#[case::ordinary_pawn_capture("e2e4 d7d5", "e4d5", b"exd5\0\0\0\0")]
#[case::check("e2e4 e7e5 d1h5 b8c6 f1c4 g8f6", "h5f7", b"Qxf7#\0\0\0")]
fn only_en_passant_gets_an_ep_square(
    #[case] moves: &str,
    #[case] uci: &str,
    #[case] san: &[u8; 8],
) {
    let pos = pos_after(moves);
    let n = written_node(&pos, uci);
    assert_eq!(san_of(&n), san);
    assert_eq!(n[10], 65, "no en passant");
}

#[test]
fn the_san_of_a_node_is_cut_at_seven_bytes_with_a_nul() {
    // A legal SAN never passes 7 bytes, so the cut is checked on the encoder.
    let node = tree_of(3, 1).nodes[0].clone();
    let extras = NodeExtras {
        san: "12345678".to_owned(),
        ..NodeExtras::default()
    };
    let mut out = Vec::new();
    encode_node(&node, &extras, &mut out);
    assert_eq!(out.len(), NODE_LEN);
    assert_eq!(&out[13..21], b"1234567\0");
}

// ------------------------------------------------------------ D1064: 32-bit sums

fn with_sum(which: usize, value: u64) -> CachedTree {
    let mut t = tree_of(5, 7);
    match which {
        0 => t.nodes[0].elo_sum = value,
        1 => t.nodes[0].perf_sum = value,
        _ => t.nodes[0].year_sum = value,
    }
    t
}

#[rstest]
#[case::elo(0)]
#[case::perf(1)]
#[case::year(2)]
fn an_entry_with_a_sum_over_u32_max_is_not_written(#[case] which: usize) {
    let (_dir, base) = tmp_base();
    let probes = positions(3);
    let mut cache = StcCache::new(Some(&base));
    cache.store(&probes[0], tree_of(5, 7));
    cache.store(&probes[1], with_sum(which, u64::from(u32::MAX) + 1));
    cache.store(&probes[2], with_sum(which, u64::from(u32::MAX))); // fits
    let size = cache.write(None).unwrap();
    assert_eq!(cache.last_write_skipped(), 1);
    assert_eq!(std::fs::metadata(stc_file(&base)).unwrap().len(), size);

    let mut fresh = StcCache::new(Some(&base));
    fresh.read().unwrap();
    assert_eq!(fresh.info().0, 2);
    assert!(fresh.lookup(&probes[0], 5).is_some());
    assert!(fresh.lookup(&probes[1], 5).is_none(), "left out");
    let kept = fresh.lookup(&probes[2], 5).unwrap();
    let sum = [
        kept.nodes[0].elo_sum,
        kept.nodes[0].perf_sum,
        kept.nodes[0].year_sum,
    ][which];
    assert_eq!(sum, u64::from(u32::MAX), "the boundary value is exact");
}

#[test]
fn the_skipped_count_follows_each_write() {
    let (_dir, base) = tmp_base();
    let probes = positions(2);
    let mut cache = StcCache::new(Some(&base));
    assert_eq!(cache.last_write_skipped(), 0);
    cache.store(&probes[0], with_sum(0, u64::MAX));
    cache.store(&probes[1], with_sum(2, u64::MAX));
    cache.write(None).unwrap();
    assert_eq!(cache.last_write_skipped(), 2);
    // Nothing skipped in a cache that fits: the count goes back to 0.
    let mut ok = StcCache::new(Some(&base));
    ok.store(&probes[0], tree_of(5, 7));
    ok.write(None).unwrap();
    assert_eq!(ok.last_write_skipped(), 0);
}

#[test]
fn a_cache_whose_only_entry_is_omitted_writes_an_empty_coherent_file() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), with_sum(1, u64::MAX));
    assert_eq!(cache.write(None).unwrap(), HEADER_LEN as u64);
    let bytes = std::fs::read(stc_file(&base)).unwrap();
    let h = StcHeader::from_bytes(&bytes).unwrap();
    assert_eq!(
        (h.num_in_use, h.lowest_total, h.lowest_total_index),
        (0, 0, 0)
    );
}

#[test]
fn after_an_omitted_entry_the_next_search_is_a_full_one() {
    let (fx, mut db) = db_of(vec![gb("e2e4 e7e5"), gb("e2e4 c7c5"), gb("d2d4 d7d5")]);
    let start = Position::new();
    let (_dir, base) = tmp_base();
    let mut writer = StcCache::new(Some(&base));
    let mut big = tree_of(3, 3);
    big.nodes[0].elo_sum = u64::from(u32::MAX) + 1;
    writer.store(&start, big);
    writer.write(None).unwrap();

    let opts = TreeOptions::default();
    let truth = run(&mut db, &start, &opts, &mut TreeState::default()).unwrap();
    let mut cache = StcCache::new(Some(&base));
    let r = run_with(
        &mut db,
        &start,
        0,
        &Filter::new(3),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert!(!r.from_cache, "the entry was never written");
    assert_eq!(sans(&r), sans(&truth));
    assert!(sans(&r).contains(&"d4"), "rows come from the database");
    // The recomputed tree is now cached.
    let again = run_with(
        &mut db,
        &start,
        0,
        &Filter::new(3),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert!(again.from_cache);
    drop(fx);
}

// ------------------------------------------------------------ D1065

#[test]
fn a_stale_entry_stays_and_the_position_is_not_cached_again() {
    // D1065 (documented behavior, not necessarily desirable): the same
    // position with another number of games does not replace the entry; it
    // only becomes the most recent one. It leaves by eviction or `clear`.
    let pos = Position::new();
    let mut cache = StcCache::new(None);
    cache.store(&pos, tree_of(5, 7));
    cache.store(&pos, tree_of(6, 9));
    assert_eq!(cache.info().0, 1, "one entry for the position");
    assert_eq!(cache.backup_info().0, 1);
    assert!(
        cache.lookup(&pos, 6).is_none(),
        "the new size is not cached"
    );
    let old = cache.lookup(&pos, 5).expect("the old entry stays");
    assert_eq!(old.nodes[0].total, 7);
    cache.clear();
    cache.store(&pos, tree_of(6, 9));
    assert!(cache.lookup(&pos, 6).is_some(), "after clear it is cached");
}

#[test]
fn a_search_on_a_grown_base_is_full_each_time_while_the_stale_entry_stays() {
    let (fx, mut db) = db_of(vec![gb("e2e4 e7e5"), gb("e2e4 c7c5"), gb("d2d4 d7d5")]);
    let start = Position::new();
    let mut cache = StcCache::new(None);
    cache.store(&start, tree_of(2, 1)); // a cache made for a base of 2 games
    for _ in 0..2 {
        let r = run_with(
            &mut db,
            &start,
            0,
            &Filter::new(3),
            &TreeOptions::default(),
            &mut TreeState::default(),
            &mut cache,
        )
        .unwrap();
        assert!(!r.from_cache);
    }
    assert_eq!(cache.info().0, 1);
    drop(fx);
}

#[test]
fn prime_report_stored_counts_calls_to_store_not_insertions() {
    let (fx, mut db) = db_of(vec![gb("e2e4 e7e5"), gb("d2d4 d7d5")]);
    let mut cache = StcCache::new(None);
    cache.store(&Position::new(), tree_of(5, 1)); // stale: 5 games, base has 2
    for _ in 0..2 {
        let r = prime_with_lines(
            &mut db,
            &mut TreeState::default(),
            &mut cache,
            &[""],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        assert_eq!((r.searches, r.cache_hits, r.stored), (1, 0, 1));
        assert_eq!(cache.info().0, 1, "nothing was inserted");
    }
    // Sanity: the standard lines on a fresh cache insert one tree per store.
    let mut fresh = StcCache::new(None);
    let r = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut fresh,
        &["", "e4"],
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(r.stored, 2);
    assert_eq!(fresh.info().0, 2);
    drop(fx);
}

// ------------------------------------------------------------ D1065 (d): encode

fn entry_with(n: usize) -> StcEntry {
    StcEntry {
        to_move: Color::White,
        board: start_board(),
        total_count: 1,
        nodes: vec![tree_of(3, 1).nodes[0].clone(); n],
        filter: CompressedFilter::compress(&Filter::new(3)),
    }
}

#[test]
fn encode_accepts_60_nodes_and_its_length_matches_encoded_len() {
    let e = entry_with(60);
    let mut out = vec![0xAA];
    e.encode(&[], &mut out).unwrap();
    assert_eq!(
        out.len() as u64,
        1 + e.encoded_len(),
        "encoded_len is the exact size"
    );
}

#[test]
fn encode_of_more_than_60_nodes_is_corrupt_and_appends_nothing() {
    let e = entry_with(61);
    let mut out = vec![1, 2, 3];
    let r = e.encode(&[], &mut out);
    assert!(matches!(r, Err(StcError::Corrupt { .. })), "{r:?}");
    assert_eq!(out, [1, 2, 3]);
}

#[test]
fn encoded_len_is_header_free_and_follows_the_layout() {
    // D1043: 1 + 64 + 4 + 4 + n * 71 + 12 + compressed filter bytes.
    let e = entry_with(2);
    assert_eq!(
        e.encoded_len(),
        (73 + 2 * NODE_LEN + 12 + e.filter.data.len()) as u64
    );
}

#[test]
fn a_node_with_a_missing_extra_is_written_with_defaults() {
    // `extras` shorter than `nodes`: the rest uses defaults, no panic.
    let e = entry_with(2);
    let mut out = Vec::new();
    e.encode(&[NodeExtras::default()], &mut out).unwrap();
    assert_eq!(out.len() as u64, e.encoded_len());
}
