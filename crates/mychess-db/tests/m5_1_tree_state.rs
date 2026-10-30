//! Contract tests of the opening tree engine, part 3: the per-database state
//! (rules 15, 16, 18, 19, 24), the fast mode, the cache hook (D949), the
//! cancellation (D942, D955f) and the invariants of the whole search.
#![allow(
    clippy::unwrap_used,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m5_1_support.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::Path;

use mychess_core::{EcoBook, GameResult, Position};
use mychess_db::pgn_db::{OpenOptions, PgnDatabase};
use mychess_db::tree::{
    FastMode, NoCache, Sort, TreeError, TreeOptions, TreeQuery, TreeState, search,
};
use mychess_db::{CancelToken, Filter, GameSource, NoProgress};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use support::{MemCache, db_of, gb, pos_after, row, run, run_with, sans};

const W: GameResult = GameResult::White;
const BL: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;

fn four_games() -> (base::Fixture, mychess_db::si4::Si4Database) {
    db_of(vec![
        gb("e2e4 e7e5 g1f3").result(W),
        gb("e2e4 c7c5").result(BL),
        gb("d2d4 d7d5").result(D),
        gb("e2e4 e7e5"),
    ])
}

fn adjust_opts() -> TreeOptions<'static> {
    TreeOptions {
        adjust: true,
        ..TreeOptions::default()
    }
}

fn bytes(f: &Filter) -> Vec<u8> {
    f.as_bytes().to_vec()
}

// ------------------------------------------------------------ first search

#[test]
fn first_search_keeps_a_copy_of_the_main_filter_as_the_database_filter() {
    let (_fx, mut db) = four_games();
    let mut main = Filter::new(4);
    main.set(2, 0);
    main.set(4, 7);
    let mut state = TreeState::default();
    assert!(state.tree_filter().is_none() && state.db_filter().is_none());
    run_with(
        &mut db,
        &Position::new(),
        0,
        &main,
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    assert_eq!(bytes(state.db_filter().unwrap()), bytes(&main));
    assert_eq!(state.tree_filter().unwrap().len(), 4);
}

#[test]
fn games_outside_the_main_filter_still_count() {
    let (_fx, mut db) = four_games();
    let mut main = Filter::new(4);
    main.fill(0);
    let r = run_with(
        &mut db,
        &Position::new(),
        0,
        &main,
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut NoCache,
    )
    .unwrap();
    assert_eq!(r.total.unwrap().total, 4);
}

#[test]
fn later_searches_do_not_replace_the_database_filter() {
    let (_fx, mut db) = four_games();
    let base = Filter::new(4);
    let mut state = TreeState::default();
    run_with(
        &mut db,
        &Position::new(),
        0,
        &base,
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    let mut other = Filter::new(4);
    other.fill(0);
    run_with(
        &mut db,
        &pos_after("e2e4"),
        1,
        &other,
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    assert_eq!(bytes(state.db_filter().unwrap()), bytes(&base));
}

#[test]
fn tree_filter_is_cleared_before_each_search() {
    let (_fx, mut db) = four_games();
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(bytes(state.tree_filter().unwrap()), vec![2, 2, 0, 2]);
    run(
        &mut db,
        &pos_after("d2d4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(bytes(state.tree_filter().unwrap()), vec![0, 0, 2, 0]);
}

// ------------------------------------------------------------ adjust

#[test]
fn adjust_is_off_by_default() {
    let (_fx, mut db) = four_games();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut TreeState::default(),
    )
    .unwrap();
    assert!(r.adjusted_filter.is_none());
}

#[test]
fn adjust_is_base_and_tree_with_the_ply_of_the_match() {
    let (_fx, mut db) = four_games();
    let mut base = Filter::new(4);
    base.set(1, 9); // the base value is irrelevant, only "in"
    base.set(4, 0);
    let r = run_with(
        &mut db,
        &pos_after("e2e4"),
        1,
        &base,
        &adjust_opts(),
        &mut TreeState::default(),
        &mut NoCache,
    )
    .unwrap();
    // Tree: games 1, 2, 4 at ply 1 (value 2). Base excludes game 4.
    assert_eq!(bytes(r.adjusted_filter.as_ref().unwrap()), vec![2, 2, 0, 0]);
}

#[test]
fn adjust_uses_the_database_filter_not_the_filter_passed_later() {
    let (_fx, mut db) = four_games();
    let base = Filter::new(4);
    let mut state = TreeState::default();
    let first = run_with(
        &mut db,
        &pos_after("e2e4"),
        1,
        &base,
        &adjust_opts(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    let adjusted = first.adjusted_filter.unwrap();
    assert_eq!(bytes(&adjusted), vec![2, 2, 0, 2]);
    // Going back to the start with the adjusted filter as main filter: the
    // combination is still with the database filter (all in).
    let second = run_with(
        &mut db,
        &Position::new(),
        0,
        &adjusted,
        &adjust_opts(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    assert_eq!(
        bytes(second.adjusted_filter.as_ref().unwrap()),
        vec![1, 1, 1, 1]
    );
}

// ------------------------------------------------------------ clean / commit

#[test]
fn clean_returns_the_database_filter_and_drops_the_tree() {
    let (_fx, mut db) = four_games();
    let mut base = Filter::new(4);
    base.set(3, 0);
    let mut state = TreeState::default();
    run_with(
        &mut db,
        &pos_after("e2e4"),
        1,
        &base,
        &adjust_opts(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    let restored = state.clean().unwrap();
    assert_eq!(bytes(&restored), bytes(&base));
    assert!(state.tree_filter().is_none());
    assert!(state.clean().is_none(), "nothing to restore twice");
}

#[test]
fn clean_without_a_search_gives_none() {
    assert!(TreeState::default().clean().is_none());
}

#[test]
fn commit_filter_without_a_search_is_an_error() {
    let mut state = TreeState::default();
    let err = state.commit_filter(&Filter::new(4)).unwrap_err();
    assert!(matches!(err, TreeError::NothingToCommit), "{err:?}");
}

#[test]
fn commit_filter_after_clean_is_an_error() {
    let (_fx, mut db) = four_games();
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    state.clean();
    let err = state.commit_filter(&Filter::new(4)).unwrap_err();
    assert!(matches!(err, TreeError::NothingToCommit), "{err:?}");
}

#[test]
fn commit_filter_makes_the_current_filter_the_new_base() {
    let (_fx, mut db) = four_games();
    let base = Filter::new(4);
    let mut state = TreeState::default();
    let r = run_with(
        &mut db,
        &pos_after("e2e4"),
        1,
        &base,
        &adjust_opts(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    let adjusted = r.adjusted_filter.unwrap();
    state.commit_filter(&adjusted).unwrap();
    assert_eq!(bytes(state.db_filter().unwrap()), bytes(&adjusted));
    assert!(state.tree_filter().is_some(), "the tree stays");
    // The next adjust combines with the committed base: game 3 stays out.
    let r = run_with(
        &mut db,
        &Position::new(),
        0,
        &adjusted,
        &adjust_opts(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    assert_eq!(bytes(r.adjusted_filter.as_ref().unwrap()), vec![1, 1, 0, 1]);
    // And clean now restores the committed filter.
    assert_eq!(bytes(&state.clean().unwrap()), bytes(&adjusted));
}

#[test]
fn commit_filter_rejects_a_filter_of_another_size() {
    let (_fx, mut db) = four_games();
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let err = state.commit_filter(&Filter::new(5)).unwrap_err();
    assert!(matches!(err, TreeError::FilterSize { .. }), "{err:?}");
    assert_eq!(state.db_filter().unwrap().len(), 4);
}

#[test]
fn invalidate_forgets_everything_and_the_next_search_takes_a_new_base() {
    let (_fx, mut db) = four_games();
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    state.invalidate();
    assert!(state.tree_filter().is_none() && state.db_filter().is_none());
    let mut base = Filter::new(4);
    base.set(1, 0);
    let r = run_with(
        &mut db,
        &Position::new(),
        0,
        &base,
        &adjust_opts(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    assert_eq!(bytes(state.db_filter().unwrap()), bytes(&base));
    assert_eq!(bytes(r.adjusted_filter.as_ref().unwrap()), vec![0, 1, 1, 1]);
}

#[test]
fn state_of_a_database_with_another_size_is_not_reused() {
    let (_fx, mut db4) = four_games();
    let mut state = TreeState::default();
    run(
        &mut db4,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let (_fx2, mut db2) = db_of(vec![gb("e2e4"), gb("d2d4")]);
    let r = run(&mut db2, &Position::new(), &adjust_opts(), &mut state).unwrap();
    assert_eq!(state.tree_filter().unwrap().len(), 2);
    assert_eq!(state.db_filter().unwrap().len(), 2);
    assert_eq!(bytes(r.adjusted_filter.as_ref().unwrap()), vec![1, 1]);
}

#[test]
fn main_filter_of_the_wrong_size_is_rejected() {
    let (_fx, mut db) = four_games();
    let mut state = TreeState::default();
    let err = run_with(
        &mut db,
        &Position::new(),
        0,
        &Filter::new(3),
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::FilterSize { .. }), "{err:?}");
    assert!(state.tree_filter().is_none());
}

// ------------------------------------------------------------ fast mode

fn transposition_db() -> (base::Fixture, mychess_db::si4::Si4Database) {
    db_of(vec![
        gb("e2e4 e7e5 g1f3 b8c6"),
        gb("g1f3 e7e5 e2e4 b8c6"),
        gb("d2d4 d7d5"),
    ])
}

fn mode(fast: FastMode) -> TreeOptions<'static> {
    TreeOptions {
        fast,
        ..TreeOptions::default()
    }
}

fn run_at(
    db: &mut impl GameSource,
    ucis: &str,
    ply: u32,
    opts: &TreeOptions<'_>,
    state: &mut TreeState,
) -> mychess_db::tree::TreeResult {
    let f = Filter::new(db.len());
    run_with(db, &pos_after(ucis), ply, &f, opts, state, &mut NoCache).unwrap()
}

#[test]
fn fast_search_examines_only_the_games_of_the_saved_filter() {
    let (_fx, mut db) = transposition_db();
    let mut state = TreeState::default();
    // Slow at 1.e4: only game 1 reaches it (the board after 1.Nf3 e5 2.e4 is
    // another position at that point).
    let slow1 = run_at(&mut db, "e2e4", 1, &mode(FastMode::Slow), &mut state);
    assert_eq!(slow1.searched, 3);
    // Fast, deeper: the transposed game 2 is not examined.
    let fast = run_at(
        &mut db,
        "e2e4 e7e5 g1f3",
        3,
        &mode(FastMode::Fast),
        &mut state,
    );
    assert_eq!(fast.searched, 1);
    assert_eq!(row(&fast, "Nc6").total, 1);
    // A full search at the same position finds both.
    let slow2 = run_at(
        &mut db,
        "e2e4 e7e5 g1f3",
        3,
        &mode(FastMode::Slow),
        &mut TreeState::default(),
    );
    assert_eq!(slow2.searched, 3);
    assert_eq!(row(&slow2, "Nc6").total, 2);
}

#[test]
fn fast_mode_on_a_fresh_state_does_a_full_search() {
    let (_fx, mut db) = transposition_db();
    let fast = run_at(
        &mut db,
        "e2e4 e7e5 g1f3",
        3,
        &mode(FastMode::Fast),
        &mut TreeState::default(),
    );
    assert_eq!(fast.searched, 3);
    assert_eq!(row(&fast, "Nc6").total, 2);
}

#[test]
fn fast_mode_falls_back_to_a_full_search_when_going_back_in_the_game() {
    let (_fx, mut db) = transposition_db();
    let mut state = TreeState::default();
    run_at(
        &mut db,
        "e2e4 e7e5 g1f3",
        3,
        &mode(FastMode::Slow),
        &mut state,
    );
    // Back to ply 1 (< saved ply 3): the copy is invalid.
    let fast = run_at(&mut db, "e2e4", 1, &mode(FastMode::Fast), &mut state);
    assert_eq!(fast.searched, 3);
    assert_eq!(row(&fast, "e5").total, 1);
}

#[test]
fn fast_mode_at_the_same_ply_uses_the_saved_filter() {
    let (_fx, mut db) = transposition_db();
    let mut state = TreeState::default();
    run_at(&mut db, "e2e4", 1, &mode(FastMode::Slow), &mut state);
    let fast = run_at(&mut db, "e2e4", 1, &mode(FastMode::Fast), &mut state);
    assert_eq!(fast.searched, 1);
}

#[test]
fn fast_then_slow_behaves_as_fast_in_search() {
    let (_fx, mut db) = transposition_db();
    let mut state = TreeState::default();
    run_at(&mut db, "e2e4", 1, &mode(FastMode::Slow), &mut state);
    let fts = run_at(
        &mut db,
        "e2e4 e7e5 g1f3",
        3,
        &mode(FastMode::FastThenSlow),
        &mut state,
    );
    assert_eq!(fts.searched, 1);
    // The application then calls Slow and gets the precise result.
    let slow = run_at(
        &mut db,
        "e2e4 e7e5 g1f3",
        3,
        &mode(FastMode::Slow),
        &mut state,
    );
    assert_eq!(slow.searched, 3);
    assert_eq!(row(&slow, "Nc6").total, 2);
}

#[test]
fn fast_search_is_not_stored_in_the_cache() {
    let (_fx, mut db) = transposition_db();
    let mut state = TreeState::default();
    let mut cache = MemCache::default();
    let f = Filter::new(3);
    run_with(
        &mut db,
        &Position::new(),
        0,
        &f,
        &mode(FastMode::Fast),
        &mut state,
        &mut cache,
    )
    .unwrap();
    assert_eq!(cache.stores, 0);
    assert_eq!(cache.len(), 0);
}

// ------------------------------------------------------------ cache

#[test]
fn slow_search_is_stored_and_the_second_one_hits_without_scanning() {
    let (_fx, mut db) = four_games();
    let mut cache = MemCache::default();
    let mut state = TreeState::default();
    let f = Filter::new(4);
    let pos = pos_after("e2e4");
    let first = run_with(
        &mut db,
        &pos,
        1,
        &f,
        &TreeOptions::default(),
        &mut state,
        &mut cache,
    )
    .unwrap();
    assert!(!first.from_cache);
    assert_eq!(first.searched, 4);
    assert_eq!((cache.stores, cache.len()), (1, 1));
    assert_eq!(cache.last_len, Some(4), "the key includes the game count");
    let tree_filter = bytes(state.tree_filter().unwrap());

    let mut state2 = TreeState::default();
    let second = run_with(
        &mut db,
        &pos,
        1,
        &f,
        &TreeOptions::default(),
        &mut state2,
        &mut cache,
    )
    .unwrap();
    assert!(second.from_cache);
    assert_eq!(second.searched, 0);
    assert_eq!(cache.stores, 1, "a hit stores nothing");
    assert_eq!(second.rows, first.rows);
    assert_eq!(second.total, first.total);
    assert_eq!(bytes(state2.tree_filter().unwrap()), tree_filter);
    assert_eq!(bytes(state2.db_filter().unwrap()), bytes(&f));
}

#[test]
fn cache_hit_recomputes_sort_hidden_and_eco() {
    let (_fx, mut db) = db_of(vec![
        gb("e2e4").result(W),
        gb("e2e4").result(W),
        gb("d2d4").result(BL),
        gb("c2c4").result(D),
    ]);
    let book = EcoBook::parse("A10 \"English\" 1.c4 *\nD00 \"Queen pawn\" 1.d4 *\n").unwrap();
    let mut cache = MemCache::default();
    let f = Filter::new(4);
    let pos = Position::new();
    let first = run_with(
        &mut db,
        &pos,
        0,
        &f,
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert_eq!(sans(&first)[0], "e4");
    assert!(first.rows.iter().all(|r| r.eco.is_none() && !r.hidden));
    let opts = TreeOptions {
        sort: Sort::Alpha,
        hide_moves: true,
        eco_book: Some(&book),
        ..TreeOptions::default()
    };
    let hit = run_with(
        &mut db,
        &pos,
        0,
        &f,
        &opts,
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert!(hit.from_cache);
    assert_eq!(sans(&hit), vec!["c4", "d4", "e4"]);
    assert!(hit.rows.iter().all(|r| r.hidden));
    assert_eq!(row(&hit, "d4").eco.unwrap().to_string(), "D00");
    assert_eq!(row(&hit, "e4").eco, None);
    assert_eq!(row(&hit, "e4").score_permille, 1000);
    assert_eq!(row(&hit, "d4").score_permille, 0);
}

#[test]
fn cache_is_keyed_by_position() {
    let (_fx, mut db) = four_games();
    let mut cache = MemCache::default();
    let f = Filter::new(4);
    let opts = TreeOptions::default();
    run_with(
        &mut db,
        &Position::new(),
        0,
        &f,
        &opts,
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    let other = run_with(
        &mut db,
        &pos_after("d2d4"),
        1,
        &f,
        &opts,
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert!(!other.from_cache);
    assert_eq!(cache.len(), 2);
}

#[test]
fn cache_hit_still_feeds_adjust_and_tree_filter() {
    let (_fx, mut db) = four_games();
    let mut cache = MemCache::default();
    let f = Filter::new(4);
    let pos = pos_after("e2e4");
    run_with(
        &mut db,
        &pos,
        1,
        &f,
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    let mut state = TreeState::default();
    let hit = run_with(&mut db, &pos, 1, &f, &adjust_opts(), &mut state, &mut cache).unwrap();
    assert!(hit.from_cache);
    assert_eq!(
        bytes(hit.adjusted_filter.as_ref().unwrap()),
        vec![2, 2, 0, 2]
    );
    assert_eq!(bytes(state.tree_filter().unwrap()), vec![2, 2, 0, 2]);
}

#[test]
fn cache_hit_in_fast_mode_is_served_from_the_cache() {
    let (_fx, mut db) = four_games();
    let mut cache = MemCache::default();
    let f = Filter::new(4);
    let pos = pos_after("e2e4");
    let slow = run_with(
        &mut db,
        &pos,
        1,
        &f,
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    let fast = run_with(
        &mut db,
        &pos,
        1,
        &f,
        &mode(FastMode::Fast),
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert!(fast.from_cache);
    assert_eq!(fast.rows, slow.rows);
}

#[test]
fn no_cache_never_reports_a_hit() {
    let (_fx, mut db) = four_games();
    let mut state = TreeState::default();
    for _ in 0..2 {
        let r = run(
            &mut db,
            &Position::new(),
            &TreeOptions::default(),
            &mut state,
        )
        .unwrap();
        assert!(!r.from_cache);
        assert_eq!(r.searched, 4);
    }
}

// ------------------------------------------------------------ cancellation

fn snapshot(state: &TreeState) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    (state.tree_filter().map(bytes), state.db_filter().map(bytes))
}

#[test]
fn already_cancelled_token_returns_cancelled_and_keeps_the_state() {
    let (_fx, mut db) = four_games();
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let before = snapshot(&state);
    let token = CancelToken::new();
    token.cancel();
    let f = Filter::new(4);
    let pos = Position::new();
    let query = TreeQuery {
        position: &pos,
        ply: 0,
        main_filter: &f,
    };
    let err = search(
        &mut db,
        &query,
        &adjust_opts(),
        &mut state,
        &mut NoCache,
        &token,
        &mut NoProgress,
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::Cancelled), "{err:?}");
    assert_eq!(snapshot(&state), before);
}

#[test]
fn cancelled_search_on_a_fresh_state_leaves_it_empty_and_does_not_store() {
    let (_fx, mut db) = four_games();
    let token = CancelToken::new();
    token.cancel();
    let f = Filter::new(4);
    let pos = Position::new();
    let query = TreeQuery {
        position: &pos,
        ply: 0,
        main_filter: &f,
    };
    let mut state = TreeState::default();
    let mut cache = MemCache::default();
    let err = search(
        &mut db,
        &query,
        &TreeOptions::default(),
        &mut state,
        &mut cache,
        &token,
        &mut NoProgress,
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::Cancelled));
    assert!(state.tree_filter().is_none() && state.db_filter().is_none());
    assert_eq!(cache.stores, 0);
}

fn big_pgn(games: usize) -> PgnDatabase {
    let mut text = String::new();
    for i in 0..games {
        let mv = if i % 2 == 0 { "e4" } else { "d4" };
        text.push_str("[Event \"?\"]\n[Result \"*\"]\n\n1. ");
        text.push_str(mv);
        text.push_str(" *\n\n");
    }
    PgnDatabase::from_reader(
        text.as_bytes(),
        Path::new("big.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .map(|(db, _)| db)
    .unwrap()
}

#[test]
fn cancel_from_the_progress_sink_in_the_middle_stops_the_search_untouched() {
    let mut db = big_pgn(5200);
    assert_eq!(db.len(), 5200);
    let mut state = TreeState::default();
    // A previous successful search so there is something to protect.
    let pos = Position::new();
    let f = Filter::new(db.len());
    let query = TreeQuery {
        position: &pos,
        ply: 0,
        main_filter: &f,
    };
    search(
        &mut db,
        &query,
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    let before = snapshot(&state);
    let token = CancelToken::new();
    let t2 = token.clone();
    let mut sink = move |done: u64, total: u64| {
        assert!(done <= total);
        if done > 0 {
            t2.cancel();
        }
    };
    let pos2 = pos_after("e2e4");
    let query2 = TreeQuery {
        position: &pos2,
        ply: 1,
        main_filter: &f,
    };
    let mut cache = MemCache::default();
    let err = search(
        &mut db,
        &query2,
        &adjust_opts(),
        &mut state,
        &mut cache,
        &token,
        &mut sink,
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::Cancelled), "{err:?}");
    assert_eq!(snapshot(&state), before);
    assert_eq!(cache.stores, 0);
}

#[test]
fn progress_is_monotonic_and_ends_with_the_whole_database() {
    let mut db = big_pgn(5200);
    let pos = Position::new();
    let f = Filter::new(db.len());
    let query = TreeQuery {
        position: &pos,
        ply: 0,
        main_filter: &f,
    };
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    let r = search(
        &mut db,
        &query,
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut NoCache,
        &CancelToken::new(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(r.total.as_ref().unwrap().total, 5200);
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    assert!(seen.iter().all(|(d, t)| d <= t && *t == 5200));
    assert_eq!(seen.last(), Some(&(5200, 5200)));
}

// ------------------------------------------------------------ invariants

const LINES: [&str; 7] = [
    "e2e4 e7e5 g1f3 b8c6",
    "e2e4 c7c5",
    "e2e4 e7e5",
    "d2d4 d7d5 c2c4",
    "d2d4 g8f6",
    "g1f3",
    "c2c4 e7e5",
];
const PREFIXES: [&str; 4] = ["", "e2e4", "e2e4 e7e5", "d2d4"];

#[derive(Debug, Clone)]
struct G {
    line: usize,
    result: usize,
    welo: u16,
    belo: u16,
    year: u16,
}

fn game_strategy() -> impl Strategy<Value = G> {
    (
        0..LINES.len(),
        0..4usize,
        prop_oneof![Just(0u16), 1500u16..2800],
        prop_oneof![Just(0u16), 1500u16..2800],
        prop_oneof![Just(0u16), 1990u16..2024],
    )
        .prop_map(|(line, result, welo, belo, year)| G {
            line,
            result,
            welo,
            belo,
            year,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn search_invariants_hold_and_match_a_naive_model(
        games in proptest::collection::vec(game_strategy(), 0..40),
        prefix in 0..PREFIXES.len(),
        sort in prop_oneof![Just(Sort::Alpha), Just(Sort::Eco), Just(Sort::Frequency), Just(Sort::Score)],
    ) {
        let results = [W, BL, D, GameResult::Unknown];
        let built: Vec<_> = games
            .iter()
            .map(|x| {
                let date = if x.year == 0 { "????.??.??".to_owned() } else { format!("{}.01.01", x.year) };
                gb(LINES[x.line]).result(results[x.result]).elo(x.welo, x.belo).date(&date)
            })
            .collect();
        let (_fx, mut db) = db_of(built);
        let pos = pos_after(PREFIXES[prefix]);
        let opts = TreeOptions { sort, ..TreeOptions::default() };
        let mut state = TreeState::default();
        let r = run(&mut db, &pos, &opts, &mut state).unwrap();

        // Naive model: games whose line starts with the prefix.
        let pre: Vec<&str> = PREFIXES[prefix].split_whitespace().collect();
        let mut model: BTreeMap<Option<String>, u32> = BTreeMap::new();
        let mut matched = 0u32;
        for x in &games {
            let toks: Vec<&str> = LINES[x.line].split_whitespace().collect();
            if toks.len() >= pre.len() && toks[..pre.len()] == pre[..] {
                matched += 1;
                *model.entry(toks.get(pre.len()).map(|s| (*s).to_owned())).or_default() += 1;
            }
        }
        let got: BTreeMap<Option<String>, u32> =
            r.rows.iter().map(|x| (x.uci.clone(), x.total)).collect();
        prop_assert_eq!(&got, &model);

        let filter = state.tree_filter().unwrap();
        prop_assert_eq!(filter.count(), matched);
        for g in filter.iter_games() {
            prop_assert_eq!(usize::from(filter.get(g)), pre.len() + 1);
        }
        match &r.total {
            None => prop_assert!(r.rows.is_empty()),
            Some(t) => {
                prop_assert_eq!(r.rows.iter().map(|x| x.total).sum::<u32>(), t.total);
                prop_assert_eq!(t.total, matched);
                prop_assert!(t.score_permille <= 1000);
            }
        }
        for x in &r.rows {
            prop_assert!(x.score_permille <= 1000);
            prop_assert_eq!(x.freq.iter().sum::<u32>(), x.total);
            let pts = x.freq[0] * 2 + x.freq[2] + x.freq[3];
            prop_assert_eq!(x.score_permille, pts * 500 / x.total);
            prop_assert!(x.total > 0);
        }
        prop_assert!(r.rows.len() <= 60);
    }
}
