//! The `.stc` cache plugged into the tree engine (M5-3a, D1045, D1048,
//! D1052, D1053 of `docs/specs/m5-3-cache-stc.md` and rules 15 and 17 of
//! `docs/specs/m5-1-motor-da-arvore.md`). Databases are synthetic SI4 files.
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

use mychess_core::{EcoBook, GameResult, Position};
use mychess_db::Filter;
use mychess_db::tree::stc::{StcCache, remove_stc};
use mychess_db::tree::{FastMode, NoCache, Sort, TreeOptions, TreeResult, TreeState};
use pretty_assertions::assert_eq;
use rstest::rstest;
use stc::{stc_file, write_stc};
use support::{db_of, gb, pos_after, run_with};

const W: GameResult = GameResult::White;
const BL: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;

fn games() -> Vec<base::B> {
    vec![
        gb("e2e4 e7e5 g1f3")
            .result(W)
            .elo(2400, 2300)
            .date("2001.01.01"),
        gb("e2e4 c7c5")
            .result(BL)
            .elo(2500, 2100)
            .date("2003.05.05"),
        gb("d2d4 d7d5").result(D).elo(2200, 2250).date("1999.09.09"),
        gb("e2e4 e7e5"),
        gb("c2c4 e7e5").result(W).elo(2000, 0),
        gb("e2e4").result(D).elo(0, 2000),
        gb("e2e4 e7e6").result(BL).date("2010.01.01"),
    ]
}

fn eco_book() -> EcoBook {
    EcoBook::parse(
        "A10 \"English\" 1.c4 *\nB00 \"King pawn\" 1.e4 *\nB20 \"Sicilian\" 1.e4 c5 *\nD00 \"Queen pawn\" 1.d4 *\n",
    )
    .unwrap()
}

fn search(
    db: &mut mychess_db::si4::Si4Database,
    pos: &Position,
    opts: &TreeOptions<'_>,
    state: &mut TreeState,
    cache: &mut dyn mychess_db::tree::TreeCache,
) -> TreeResult {
    let filter = Filter::new(mychess_db::GameSource::len(db));
    run_with(db, pos, 0, &filter, opts, state, cache).unwrap()
}

fn plain(
    db: &mut mychess_db::si4::Si4Database,
    pos: &Position,
    opts: &TreeOptions<'_>,
) -> TreeResult {
    search(db, pos, opts, &mut TreeState::default(), &mut NoCache)
}

/// The parts of two results that must be equal whether or not the cache was
/// used.
fn same(a: &TreeResult, b: &TreeResult) {
    assert_eq!(a.rows, b.rows);
    assert_eq!(a.total, b.total);
    assert_eq!(a.turn, b.turn);
    assert_eq!(a.adjusted_filter, b.adjusted_filter);
}

fn cache_for(fx: &base::Fixture) -> StcCache {
    StcCache::new(Some(&fx.si4()))
}

// ------------------------------------------------------------ storing

#[test]
fn a_slow_search_stores_in_both_layers_and_a_repeat_hits() {
    let (fx, mut db) = db_of(games());
    let mut cache = cache_for(&fx);
    let opts = TreeOptions::default();
    let first = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(!first.from_cache);
    assert_eq!((cache.info().0, cache.backup_info().0), (1, 1));
    let second = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(second.from_cache);
    assert_eq!(
        (cache.info().0, cache.backup_info().0),
        (1, 1),
        "no new entry"
    );
    same(&first, &second);
}

#[test]
fn a_fast_search_does_not_write_to_the_cache() {
    let (fx, mut db) = db_of(games());
    let mut cache = cache_for(&fx);
    let opts = TreeOptions {
        fast: FastMode::Fast,
        ..TreeOptions::default()
    };
    let r = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(!r.from_cache);
    assert_eq!((cache.info().0, cache.backup_info().0), (0, 0));
}

#[test]
fn every_position_searched_slowly_is_stored_once() {
    let (fx, mut db) = db_of(games());
    let mut cache = cache_for(&fx);
    let opts = TreeOptions::default();
    for ucis in ["", "e2e4", "e2e4 e7e5", "d2d4"] {
        search(
            &mut db,
            &pos_after(ucis),
            &opts,
            &mut TreeState::default(),
            &mut cache,
        );
    }
    assert_eq!(cache.info().0, 4);
    // totals: start 7, e4 5, e4 e5 2, d4 1 -> the lowest is the 4th.
    assert_eq!(cache.main_lowest(), Some((1, 3)));
}

// ------------------------------------------------------------ persistence

#[test]
fn write_then_a_new_instance_answers_from_the_file() {
    let (fx, mut db) = db_of(games());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx);
    let fresh = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    cache.write(None).unwrap();

    let mut reloaded = cache_for(&fx);
    let mut state = TreeState::default();
    let hit = search(&mut db, &Position::new(), &opts, &mut state, &mut reloaded);
    assert!(hit.from_cache);
    same(&fresh, &hit);
    same(&plain(&mut db, &Position::new(), &opts), &hit);
    // The tree filter comes back from the file too.
    let mut state_full = TreeState::default();
    plain_state(&mut db, &Position::new(), &opts, &mut state_full);
    assert_eq!(state.tree_filter(), state_full.tree_filter());
}

fn plain_state(
    db: &mut mychess_db::si4::Si4Database,
    pos: &Position,
    opts: &TreeOptions<'_>,
    state: &mut TreeState,
) -> TreeResult {
    search(db, pos, opts, state, &mut NoCache)
}

#[rstest]
#[case::frequency(Sort::Frequency)]
#[case::alpha(Sort::Alpha)]
#[case::score(Sort::Score)]
#[case::eco(Sort::Eco)]
fn score_eco_and_order_are_recalculated_on_a_hit(#[case] sort: Sort) {
    let (fx, mut db) = db_of(games());
    let book = eco_book();
    // Stored with one sort order, answered with another.
    let mut cache = cache_for(&fx);
    search(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    );
    cache.write(Some(&book)).unwrap();

    let opts = TreeOptions {
        sort,
        eco_book: Some(&book),
        ..TreeOptions::default()
    };
    let mut reloaded = cache_for(&fx);
    let hit = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut reloaded,
    );
    assert!(hit.from_cache);
    same(&plain(&mut db, &Position::new(), &opts), &hit);
    let e4 = hit.rows.iter().find(|r| r.san == "e4").unwrap();
    assert_eq!(e4.eco.map(|e| e.to_string()).as_deref(), Some("B00"));
}

#[test]
fn eco_and_ordering_follow_the_options_of_the_hit_not_those_of_the_file() {
    let (fx, mut db) = db_of(games());
    let book = eco_book();
    let mut cache = cache_for(&fx);
    // Written WITH the eco book...
    search(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    );
    cache.write(Some(&book)).unwrap();
    // ...answered WITHOUT one.
    let mut reloaded = cache_for(&fx);
    let hit = search(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut reloaded,
    );
    assert!(hit.from_cache);
    assert!(hit.rows.iter().all(|r| r.eco.is_none()));
}

#[test]
fn san_score_and_eco_code_poisoned_in_the_file_are_ignored() {
    let (fx, mut db) = db_of(games());
    let opts = TreeOptions {
        sort: Sort::Score,
        ..TreeOptions::default()
    };
    let mut cache = cache_for(&fx);
    search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    cache.write(None).unwrap();

    let path = stc_file(&fx.base);
    let mut bytes = std::fs::read(&path).unwrap();
    let moves = u32::from_be_bytes(bytes[87..91].try_into().unwrap()) as usize;
    assert!(moves >= 3, "e4, d4, c4 at least");
    for i in 0..moves {
        let n = 95 + 71 * i;
        bytes[n + 13..n + 21].copy_from_slice(b"POISON!!");
        bytes[n + 41..n + 45].copy_from_slice(&[0xFF; 4]);
        bytes[n + 45..n + 47].copy_from_slice(&[0xFF; 2]);
        bytes[n] = 200;
        bytes[n + 1] = 200;
    }
    write_stc(&fx.base, &bytes);

    let mut reloaded = cache_for(&fx);
    let hit = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut reloaded,
    );
    assert!(hit.from_cache);
    same(&plain(&mut db, &Position::new(), &opts), &hit);
    assert!(hit.rows.iter().all(|r| r.san != "POISON!!"));
}

#[test]
fn adjust_after_a_hit_gives_the_same_filter_as_after_a_full_search() {
    let (fx, mut db) = db_of(games());
    let opts = TreeOptions {
        adjust: true,
        ..TreeOptions::default()
    };
    let pos = pos_after("e2e4");
    let mut cache = cache_for(&fx);
    search(&mut db, &pos, &opts, &mut TreeState::default(), &mut cache);
    cache.write(None).unwrap();
    let mut reloaded = cache_for(&fx);
    let hit = search(
        &mut db,
        &pos,
        &opts,
        &mut TreeState::default(),
        &mut reloaded,
    );
    assert!(hit.from_cache);
    let full = plain(&mut db, &pos, &opts);
    assert!(full.adjusted_filter.is_some());
    same(&full, &hit);
}

#[test]
fn a_position_that_is_not_in_the_file_is_searched_in_full() {
    let (fx, mut db) = db_of(games());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx);
    search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    cache.write(None).unwrap();
    let mut reloaded = cache_for(&fx);
    let r = search(
        &mut db,
        &pos_after("e2e4"),
        &opts,
        &mut TreeState::default(),
        &mut reloaded,
    );
    assert!(!r.from_cache);
    same(&plain(&mut db, &pos_after("e2e4"), &opts), &r);
    assert_eq!(reloaded.info().0, 2, "stored next to the loaded one");
}

#[test]
fn a_transposition_hits_because_the_key_ignores_the_move_order() {
    let (fx, mut db) = db_of(games());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx);
    search(
        &mut db,
        &pos_after("g1f3 g8f6 b1c3"),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    let r = search(
        &mut db,
        &pos_after("b1c3 g8f6 g1f3"),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(r.from_cache);
}

// ------------------------------------------------------------ validity

#[test]
fn a_cache_from_another_number_of_games_is_not_used() {
    let (fx3, mut db3) = db_of(games().into_iter().take(3).collect());
    let (_fx7, mut db7) = db_of(games());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx3);
    search(
        &mut db3,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert_eq!(cache.info().0, 1);

    // The same cache object, used on a database with more games.
    let r = search(
        &mut db7,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(!r.from_cache, "3-game tree must not answer for 7 games");
    same(&plain(&mut db7, &Position::new(), &opts), &r);
    assert_eq!(r.total.as_ref().unwrap().total, 7);
}

#[test]
fn a_file_written_for_fewer_games_is_ignored_after_games_are_added() {
    let (fx3, mut db3) = db_of(games().into_iter().take(3).collect());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx3);
    search(
        &mut db3,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    cache.write(None).unwrap();
    let bytes = std::fs::read(stc_file(&fx3.base)).unwrap();

    let (fx7, mut db7) = db_of(games());
    write_stc(&fx7.base, &bytes);
    let mut reloaded = cache_for(&fx7);
    let r = search(
        &mut db7,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut reloaded,
    );
    assert!(!r.from_cache);
    assert_eq!(r.total.as_ref().unwrap().total, 7);
}

#[test]
fn the_cache_has_no_database_signature_so_same_length_content_hits() {
    // D1045: only CFilterSize == len is checked. Invalidating after an edit
    // is the job of clear() + remove_stc().
    let (fx_a, mut db_a) = db_of(games());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx_a);
    search(
        &mut db_a,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );

    let mut other = games();
    other[0] = gb("d2d4 d7d5 c2c4").result(W);
    let (_fx_b, mut db_b) = db_of(other);
    let stale = search(
        &mut db_b,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(stale.from_cache);
    cache.clear();
    let fresh = search(
        &mut db_b,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(!fresh.from_cache);
}

#[test]
fn after_clear_and_remove_stc_the_next_search_is_a_full_one() {
    let (fx, mut db) = db_of(games());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx);
    search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    cache.write(None).unwrap();
    cache.clear();
    assert!(remove_stc(&fx.base).unwrap());
    let r = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(!r.from_cache);
}

// ------------------------------------------------------------ tolerance

#[test]
fn an_invalid_cache_file_does_not_fail_the_search() {
    let (fx, mut db) = db_of(games());
    write_stc(&fx.base, b"this is not a tree cache");
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx);
    let r = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(!r.from_cache);
    same(&plain(&mut db, &Position::new(), &opts), &r);
    assert!(cache.take_load_error().is_some());
    assert_eq!(cache.info().0, 1, "the search stored its own tree");
}

#[test]
fn a_truncated_cache_file_does_not_fail_the_search_at_any_cut() {
    let (fx, mut db) = db_of(games());
    let opts = TreeOptions::default();
    let mut cache = cache_for(&fx);
    for ucis in ["", "e2e4"] {
        search(
            &mut db,
            &pos_after(ucis),
            &opts,
            &mut TreeState::default(),
            &mut cache,
        );
    }
    cache.write(None).unwrap();
    let bytes = std::fs::read(stc_file(&fx.base)).unwrap();
    let expected = plain(&mut db, &Position::new(), &opts);
    for n in (0..bytes.len()).step_by(7) {
        write_stc(&fx.base, &bytes[..n]);
        let mut c = cache_for(&fx);
        let r = search(
            &mut db,
            &Position::new(),
            &opts,
            &mut TreeState::default(),
            &mut c,
        );
        assert!(!r.from_cache, "cut at {n}");
        same(&expected, &r);
    }
}

#[test]
fn a_missing_file_is_a_plain_miss() {
    let (fx, mut db) = db_of(games());
    let mut cache = cache_for(&fx);
    let r = search(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(!r.from_cache);
    assert!(cache.take_load_error().is_none());
}

#[test]
fn a_cache_without_a_database_file_works_in_memory_only() {
    let (_fx, mut db) = db_of(games());
    let mut cache = StcCache::new(None);
    let opts = TreeOptions::default();
    search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    let r = search(
        &mut db,
        &Position::new(),
        &opts,
        &mut TreeState::default(),
        &mut cache,
    );
    assert!(r.from_cache);
    assert_eq!(cache.write(None).unwrap(), 0);
}

// ------------------------------------------------------------ the Scid's own file

fn reference_stc_files() -> Vec<std::path::PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("stc"))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
#[ignore = "requires a real Scid .stc under reference/scid_vs_pc-4.27 (none ships with it): run with `cargo xtask compat`"]
fn compat_real_scid_stc_files_are_read_and_agree_with_a_full_search() {
    let files = reference_stc_files();
    assert!(
        !files.is_empty(),
        "no .stc fixture in reference/: the byte layout is covered by the hand-built goldens"
    );
    for stc in files {
        let base = stc.with_extension("si4");
        if !base.exists() {
            continue;
        }
        let mut db = mychess_db::si4::Si4Database::open(&base).unwrap();
        let mut cache = StcCache::new(Some(&base));
        cache.read().unwrap();
        assert!(cache.info().0 > 0);
        let opts = TreeOptions::default();
        let hit = search(
            &mut db,
            &Position::new(),
            &opts,
            &mut TreeState::default(),
            &mut cache,
        );
        let full = plain(&mut db, &Position::new(), &opts);
        same(&full, &hit);
    }
}
