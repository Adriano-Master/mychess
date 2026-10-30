//! Replacement policies, capacities, validity and the two layers of the
//! cache (M5-3a, D1045 to D1047, D1052 and D1061 of
//! `docs/specs/m5-3-cache-stc.md`). Only the public `TreeCache` interface and
//! the inspection methods are used; the contents of the main layer alone are
//! observed through a write to a scratch file and a read by a fresh cache
//! (whose backup layer is empty).
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
use mychess_db::tree::TreeCache;
use mychess_db::tree::stc::{
    BACKUP_CACHE_SIZE, DEFAULT_CACHE_SIZE, MAX_CACHE_SIZE, Policy, StcCache,
};
use pretty_assertions::assert_eq;
use stc::{
    file, header, main_has, positions, simple_entry, stc_file, tmp_base, tree_of, write_stc,
};

const LEN: u32 = 4;

fn store(cache: &mut StcCache, pos: &Position, total: u32) {
    cache.store(pos, tree_of(LEN, total));
}

fn total_of(cache: &mut StcCache, pos: &Position) -> Option<u32> {
    cache.lookup(pos, LEN).map(|t| t.nodes[0].total)
}

// ------------------------------------------------------------ capacities

#[test]
fn default_capacities_are_1000_main_and_100_backup() {
    assert_eq!(DEFAULT_CACHE_SIZE, 1000);
    assert_eq!(BACKUP_CACHE_SIZE, 100);
    assert_eq!(MAX_CACHE_SIZE, 100_000);
    let cache = StcCache::new(None);
    assert_eq!(cache.info(), (0, 1000));
    assert_eq!(cache.backup_info(), (0, 100));
    assert_eq!(cache.main_lowest(), None);
}

#[test]
fn both_layers_fill_up_to_their_capacity_and_no_further() {
    let mut cache = StcCache::new(None);
    let ps = positions(1010);
    for (i, p) in ps.iter().enumerate() {
        store(&mut cache, p, 100 + u32::try_from(i).unwrap());
    }
    assert_eq!(cache.info(), (1000, 1000));
    assert_eq!(cache.backup_info(), (100, 100));
}

#[test]
fn a_full_default_cache_rejects_a_tree_smaller_than_the_lowest() {
    let mut cache = StcCache::new(None);
    let ps = positions(1002);
    for (i, p) in ps.iter().take(1000).enumerate() {
        store(&mut cache, p, 100 + u32::try_from(i).unwrap());
    }
    assert_eq!(cache.main_lowest(), Some((100, 0)));
    store(&mut cache, &ps[1000], 99);
    assert_eq!(cache.main_lowest(), Some((100, 0)));
    assert_eq!(cache.info().0, 1000);
    // A bigger one evicts the lowest (index 0) and the next lowest follows.
    store(&mut cache, &ps[1001], 5000);
    assert_eq!(cache.main_lowest(), Some((101, 1)));
}

// ------------------------------------------------------------ Smallest

#[test]
fn smallest_rejects_lower_replaces_on_a_tie_and_evicts_the_lowest() {
    let ps = positions(10);
    let mut cache = StcCache::new(None);
    cache.resize(3);
    store(&mut cache, &ps[0], 50); // index 0
    store(&mut cache, &ps[1], 10); // index 1
    store(&mut cache, &ps[2], 30); // index 2
    assert_eq!(cache.info(), (3, 3));
    assert_eq!(cache.main_lowest(), Some((10, 1)));

    // Lower than the lowest: rejected.
    store(&mut cache, &ps[3], 9);
    assert_eq!(cache.main_lowest(), Some((10, 1)));
    assert_eq!(main_has(&cache, &ps[..4], LEN), [true, true, true, false]);

    // Equal to the lowest: replaces it.
    store(&mut cache, &ps[4], 10);
    assert_eq!(
        main_has(&cache, &ps[..5], LEN),
        [true, false, true, false, true]
    );
    assert_eq!(cache.main_lowest(), Some((10, 1)));

    // Bigger: evicts the lowest, which is recomputed over all the entries.
    store(&mut cache, &ps[5], 40);
    assert_eq!(
        main_has(&cache, &ps[..6], LEN),
        [true, false, true, false, false, true]
    );
    assert_eq!(cache.main_lowest(), Some((30, 2)));
    assert_eq!(cache.info(), (3, 3));
}

#[test]
fn the_new_lowest_is_recomputed_over_every_entry_with_ties_to_the_lowest_index() {
    // D1046: the Scid left LowestTotalIndex on the new tree when it tied with
    // an older one at a lower index.
    let ps = positions(10);
    let mut cache = StcCache::new(None);
    cache.resize(3);
    store(&mut cache, &ps[0], 20); // 0
    store(&mut cache, &ps[1], 10); // 1
    store(&mut cache, &ps[2], 20); // 2
    assert_eq!(cache.main_lowest(), Some((10, 1)));
    store(&mut cache, &ps[3], 20); // replaces index 1: [20, 20, 20]
    assert_eq!(cache.main_lowest(), Some((20, 0)));
    // So the next eviction takes index 0, not index 1.
    store(&mut cache, &ps[4], 30);
    assert_eq!(
        main_has(&cache, &ps[..5], LEN),
        [false, false, true, true, true]
    );
    // [30, 20, 20] -> lowest 20 at index 1
    assert_eq!(cache.main_lowest(), Some((20, 1)));
}

#[test]
fn the_new_tree_can_itself_be_the_lowest_after_an_eviction() {
    let ps = positions(10);
    let mut cache = StcCache::new(None);
    cache.resize(3);
    store(&mut cache, &ps[0], 50);
    store(&mut cache, &ps[1], 10);
    store(&mut cache, &ps[2], 30);
    store(&mut cache, &ps[3], 20); // replaces index 1 and is the new lowest
    assert_eq!(cache.main_lowest(), Some((20, 1)));
}

#[test]
fn an_existing_position_is_not_replaced_or_resorted() {
    let ps = positions(3);
    let mut cache = StcCache::new(None);
    store(&mut cache, &ps[0], 50);
    store(&mut cache, &ps[1], 10);
    store(&mut cache, &ps[0], 999);
    assert_eq!(total_of(&mut cache, &ps[0]), Some(50), "first tree kept");
    assert_eq!(cache.info().0, 2);
    assert_eq!(cache.backup_info().0, 2);
    assert_eq!(cache.main_lowest(), Some((10, 1)));
}

#[test]
fn a_hit_does_not_reorder_or_change_the_lowest() {
    let ps = positions(4);
    let mut cache = StcCache::new(None);
    cache.resize(2);
    store(&mut cache, &ps[0], 10);
    store(&mut cache, &ps[1], 20);
    for _ in 0..5 {
        assert!(cache.lookup(&ps[0], LEN).is_some());
    }
    assert_eq!(cache.main_lowest(), Some((10, 0)));
    store(&mut cache, &ps[2], 15); // evicts index 0 despite the hits
    assert_eq!(main_has(&cache, &ps[..3], LEN), [false, true, true]);
}

#[test]
fn capacity_one_keeps_the_biggest_tree() {
    let ps = positions(4);
    let mut cache = StcCache::new(None);
    cache.resize(1);
    store(&mut cache, &ps[0], 10);
    store(&mut cache, &ps[1], 5);
    store(&mut cache, &ps[2], 10);
    store(&mut cache, &ps[3], 11);
    assert_eq!(main_has(&cache, &ps, LEN), [false, false, false, true]);
    assert_eq!(cache.main_lowest(), Some((11, 0)));
}

// ------------------------------------------------------------ Oldest (backup)

/// A cache whose main layer has capacity 1 and holds a huge tree (position
/// 0), so every other tree is rejected there and only the backup layer
/// (Oldest, 100 slots) serves them.
fn backup_only() -> (StcCache, Vec<Position>) {
    let ps = positions(260);
    let mut cache = StcCache::new(None);
    cache.resize(1);
    store(&mut cache, &ps[0], 1_000_000);
    (cache, ps)
}

fn live(cache: &mut StcCache, ps: &[Position]) -> Vec<bool> {
    ps.iter().map(|p| cache.lookup(p, LEN).is_some()).collect()
}

#[test]
fn oldest_fills_the_100_slots_then_evicts_from_index_1_as_a_ring() {
    let (mut cache, ps) = backup_only();
    for p in &ps[1..100] {
        store(&mut cache, p, 1);
    }
    assert_eq!(cache.backup_info(), (100, 100));
    assert_eq!(cache.info(), (1, 1));
    assert!(live(&mut cache, &ps[..100]).iter().all(|b| *b));

    // D1061: the mark starts at 0, so the first eviction is index 1 (ps[1]).
    store(&mut cache, &ps[100], 1);
    let l = live(&mut cache, &ps[..101]);
    assert!(!l[1], "ps[1] evicted first");
    assert!(l[0] && l[2] && l[99] && l[100]);

    store(&mut cache, &ps[101], 1);
    let l = live(&mut cache, &ps[..102]);
    assert!(!l[1] && !l[2], "then ps[2]");
    assert!(l[3] && l[101]);
}

#[test]
fn oldest_wraps_around_after_one_full_turn() {
    let (mut cache, ps) = backup_only();
    for p in &ps[1..200] {
        store(&mut cache, p, 1);
    }
    // 99 trees took indexes 1..=99 (the 100th slot, index 0, holds ps[0]).
    // ps[100..=198] replaced 1..=99, and ps[199]... the 100th eviction wraps
    // to index 0, taking ps[0]'s slot in the backup (ps[0] stays in main).
    let l = live(&mut cache, &ps[..200]);
    assert!(l[0], "ps[0] is still served by the main layer");
    assert!(l[1..100].iter().all(|b| !b), "the first generation is gone");
    assert!(
        l[100..200].iter().all(|b| *b),
        "the second generation is live"
    );
    assert_eq!(cache.backup_info(), (100, 100));
}

#[test]
fn an_existing_position_moves_the_ring_mark_to_its_slot() {
    let (mut cache, ps) = backup_only();
    for p in &ps[1..100] {
        store(&mut cache, p, 1);
    }
    store(&mut cache, &ps[50], 1); // already there: mark = 50
    store(&mut cache, &ps[100], 1); // evicts index 51
    let l = live(&mut cache, &ps[..101]);
    assert!(!l[51], "slot after the mark is taken");
    assert!(l[1] && l[50] && l[52] && l[100]);
}

// ------------------------------------------------------------ the two layers

#[test]
fn the_main_layer_is_consulted_before_the_backup() {
    let (_dir, base) = tmp_base();
    let p = Position::new();
    // The main layer comes from a file with a tree for 5 games.
    let mut a = StcCache::new(Some(&base));
    a.store(&p, tree_of(5, 7));
    a.write(None).unwrap();

    let mut cache = StcCache::new(Some(&base));
    // Loads the file lazily (main has the 5-game tree) and puts a 6-game
    // tree for the same position in the backup only.
    assert!(
        cache
            .lookup(
                &Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap(),
                5
            )
            .is_none()
    );
    cache.store(&p, tree_of(6, 9));
    assert_eq!(cache.info().0, 1);
    assert_eq!(cache.backup_info().0, 1);

    assert_eq!(cache.lookup(&p, 5).unwrap().nodes[0].total, 7, "main");
    assert_eq!(cache.lookup(&p, 6).unwrap().nodes[0].total, 9, "backup");
    assert!(cache.lookup(&p, 7).is_none(), "neither");

    // With the main layer emptied, the backup serves.
    cache.resize(1);
    assert_eq!(cache.info().0, 0);
    assert_eq!(cache.lookup(&p, 6).unwrap().nodes[0].total, 9);
    assert!(cache.lookup(&p, 5).is_none());
}

#[test]
fn store_goes_to_both_layers() {
    let mut cache = StcCache::new(None);
    store(&mut cache, &Position::new(), 3);
    assert_eq!((cache.info().0, cache.backup_info().0), (1, 1));
}

#[test]
fn only_the_main_layer_reaches_the_file() {
    let (_dir, base) = tmp_base();
    let (mut cache, ps) = backup_only();
    for p in &ps[1..30] {
        store(&mut cache, p, 1);
    }
    assert_eq!(cache.backup_info().0, 30);
    let size = cache.write_to(&base, None).unwrap();
    let mut fresh = StcCache::new(Some(&base));
    assert!(fresh.lookup(&ps[0], LEN).is_some());
    assert!(fresh.lookup(&ps[5], LEN).is_none());
    assert_eq!(fresh.info(), (1, 1));
    assert_eq!(size, std::fs::metadata(stc_file(&base)).unwrap().len());
}

// ------------------------------------------------------------ validity

#[test]
fn a_hit_needs_the_filter_size_to_equal_the_number_of_games() {
    let mut cache = StcCache::new(None);
    let p = Position::new();
    cache.store(&p, tree_of(5, 7));
    assert!(cache.lookup(&p, 5).is_some());
    for len in [0u32, 1, 4, 6, 100, u32::MAX] {
        assert!(cache.lookup(&p, len).is_none(), "len {len}");
    }
}

#[test]
fn validity_also_holds_for_entries_read_from_the_file() {
    let (_dir, base) = tmp_base();
    let p = Position::new();
    let mut a = StcCache::new(Some(&base));
    a.store(&p, tree_of(5, 7));
    a.write(None).unwrap();
    let mut b = StcCache::new(Some(&base));
    assert!(b.lookup(&p, 6).is_none());
    assert!(b.lookup(&p, 5).is_some());
}

// ------------------------------------------------------------ keys

#[test]
fn the_key_is_side_to_move_plus_the_64_squares() {
    let mut cache = StcCache::new(None);
    let w = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let b = Position::from_fen("4k3/8/8/8/8/8/8/4K3 b - - 0 1").unwrap();
    cache.store(&w, tree_of(2, 1));
    assert!(cache.lookup(&b, 2).is_none(), "other side to move");
    cache.store(&b, tree_of(2, 2));
    assert_eq!(cache.info().0, 2);
    assert_eq!(cache.lookup(&b, 2).unwrap().nodes[0].total, 2);
}

#[test]
fn castling_rights_and_en_passant_are_not_part_of_the_key() {
    let mut cache = StcCache::new(None);
    let full = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let none = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1").unwrap();
    cache.store(&full, tree_of(2, 1));
    assert_eq!(cache.lookup(&none, 2).unwrap().nodes[0].total, 1);
    cache.store(&none, tree_of(2, 9));
    assert_eq!(cache.info().0, 1, "same key");

    let ep =
        Position::from_fen("rnbqkbnr/ppp1pppp/8/8/3pP3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
    let no_ep =
        Position::from_fen("rnbqkbnr/ppp1pppp/8/8/3pP3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
    cache.store(&ep, tree_of(2, 4));
    assert!(cache.lookup(&no_ep, 2).is_some());
}

// ------------------------------------------------------------ resize / clear

#[test]
fn shrinking_empties_the_main_layer_only_and_growing_keeps_its_trees() {
    let ps = positions(3);
    let mut cache = StcCache::new(None);
    for p in &ps {
        store(&mut cache, p, 5);
    }
    cache.resize(2000);
    assert_eq!(cache.info(), (3, 2000));
    cache.resize(2000);
    assert_eq!(cache.info(), (3, 2000), "same size keeps");
    cache.resize(1000);
    assert_eq!(cache.info(), (0, 1000), "smaller empties");
    assert_eq!(cache.backup_info(), (3, 100), "backup untouched");
    assert_eq!(cache.main_lowest(), None);
}

#[test]
fn shrinking_does_not_remove_the_file() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(2, 1));
    cache.write(None).unwrap();
    cache.resize(250);
    assert!(stc_file(&base).exists());
}

#[test]
fn resize_is_limited_to_1_to_100000() {
    let mut cache = StcCache::new(None);
    cache.resize(0);
    assert_eq!(cache.info().1, 1);
    cache.resize(100_000);
    assert_eq!(cache.info().1, 100_000);
    cache.resize(100_001);
    assert_eq!(cache.info().1, 100_000);
    cache.resize(usize::MAX);
    assert_eq!(cache.info().1, 100_000);
    cache.resize(250);
    assert_eq!(cache.info().1, 250);
}

#[test]
fn the_documented_size_options_are_all_accepted() {
    let mut cache = StcCache::new(None);
    for size in [250usize, 500, 1000, 2000, 5000, 10_000] {
        cache.resize(size);
        assert_eq!(cache.info().1, size);
    }
}

#[test]
fn clear_empties_both_layers_but_keeps_the_capacities() {
    let mut cache = StcCache::new(None);
    cache.resize(77);
    for p in &positions(5) {
        store(&mut cache, p, 3);
    }
    cache.clear();
    assert_eq!(cache.info(), (0, 77));
    assert_eq!(cache.backup_info(), (0, 100));
    assert_eq!(cache.main_lowest(), None);
    assert!(cache.lookup(&Position::new(), LEN).is_none());
}

#[test]
fn the_cache_size_of_the_file_replaces_the_configured_one_and_governs_eviction() {
    let (_dir, base) = tmp_base();
    let mut second = simple_entry(3, 8);
    second[0] = 1;
    write_stc(
        &base,
        &file(header(2, 2, 5, 0), &[simple_entry(3, 5), second]),
    );
    let mut cache = StcCache::new(Some(&base));
    cache.resize(500);
    assert!(cache.lookup(&Position::new(), 3).is_some());
    assert_eq!(cache.info(), (2, 2), "capacity from the file");
    // Full: a smaller tree is rejected, a bigger one evicts the lowest (5).
    let ps = positions(2);
    cache.store(&ps[0], tree_of(3, 1));
    assert_eq!(cache.info().0, 2);
    assert_eq!(main_has(&cache, &ps[..1], 3), [false]);
    cache.store(&ps[1], tree_of(3, 50));
    assert_eq!(main_has(&cache, &ps[1..], 3), [true]);
    assert_eq!(cache.main_lowest(), Some((8, 1)));
}

#[test]
fn policy_enum_has_the_two_policies_of_the_scid() {
    assert_ne!(Policy::Smallest, Policy::Oldest);
}
