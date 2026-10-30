//! Contract tests of the opening tree engine, part 5: damaged records (D957,
//! D955(g)) and the "nothing changes on failure" revisions of D958.
//!
//! The synthetic records are written byte by byte from `docs/formatos/si4.md`
//! (section 9.3-9.4): `0xCF` is 1.e4 / 1...e5 (pawn number 12, double step),
//! `0x67` is Ng1-f3 (piece 6, +15), `0x22` is Nb8-c6 (piece 2, -15), `0x0F`
//! ends the game, `0x0D` opens a variation and `0x0C` marks a comment.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/si4_builder.rs"]
mod si4_builder;
#[path = "common/m5_1_support.rs"]
mod support;

use mychess_core::{Game, Position};
use mychess_db::tree::{
    BestOptions, BestOrder, FastMode, TreeError, TreeOptions, TreeQuery, TreeResult, TreeState,
    best_games, search,
};
use mychess_db::{CancelToken, Filter, GameSource, NoProgress, PlayerStats};
use pretty_assertions::assert_eq;
use si4_builder::{
    EntrySpec, IndexHeader, MemDb, Parts, default_names, index_file, open_parts, rec,
};
use support::{MemCache, db_of, gb, pos_after, run, run_with, sans};

// ------------------------------------------------------------ synthetic records

const E4: u8 = 0xCF;
const D4: u8 = 0xBF;
const NF3: u8 = 0x67;
const NC6: u8 = 0x22;
const END_GAME: u8 = 0x0F;
const START_VARIATION: u8 = 0x0D;
const COMMENT: u8 = 0x0C;

/// An in-memory database whose entries all say "4 half moves" (so that the
/// search does not skip them as empty games).
fn mem_db(records: &[Vec<u8>]) -> MemDb {
    let mut sg4 = Vec::new();
    let mut entries = Vec::new();
    for r in records {
        let mut e = EntrySpec::new(sg4.len() as u32, r.len() as u32);
        e.half_moves = 4;
        entries.push(e.encode());
        sg4.extend_from_slice(r);
    }
    let parts = Parts {
        index: index_file(&IndexHeader::default(), &entries),
        names: default_names(),
        sg4,
    };
    open_parts(&parts).unwrap()
}

fn stream(bytes: &[u8]) -> Vec<u8> {
    rec(bytes, &[])
}

/// A record without the end-of-game token (the file ends in the moves).
fn truncated(bytes: &[u8]) -> Vec<u8> {
    let mut r = rec(&[], &[]);
    r.extend_from_slice(bytes);
    r
}

/// Records 1-6 are damaged only AFTER 1.e4 e5 2.Nf3; 7-9 are damaged BEFORE
/// that point; 10 is intact and never reaches the position.
fn damaged_records() -> Vec<Vec<u8>> {
    let mut bad_comments = stream(&[E4, E4, NF3, NC6, COMMENT, END_GAME]);
    bad_comments.extend_from_slice(b"ab"); // the comment has no terminator
    vec![
        // 1: intact control
        stream(&[E4, E4, NF3, NC6, END_GAME]),
        // 2: the move after the match has an invalid code (black knight, v 0)
        stream(&[E4, E4, NF3, 0x20, END_GAME]),
        // 3: the move after the match is illegal (Ra8xb8, own knight)
        stream(&[E4, E4, NF3, 0x11, END_GAME]),
        // 4: the stream is cut right after the match
        truncated(&[E4, E4, NF3]),
        // 5: a variation that never ends, right after the matching move
        stream(&[E4, E4, NF3, START_VARIATION, NC6, END_GAME]),
        // 6: the comment section is truncated
        bad_comments,
        // 7: an unterminated variation BEFORE the match (after 1.e4)
        stream(&[E4, START_VARIATION, NC6, END_GAME]),
        // 8: an illegal move before the match (White Ra1xb1, own knight)
        stream(&[0x11, E4, E4, NF3, END_GAME]),
        // 9: the stream is cut before the match (after 1.e4)
        truncated(&[E4]),
        // 10: 1.d4, intact, never reaches the target
        stream(&[D4, END_GAME]),
    ]
}

#[test]
fn the_damaged_records_are_really_damaged_for_a_full_decoder() {
    // Guards the fixture: if the full decoder tolerated these, the tests
    // below would prove nothing about the "only the main line" scan.
    let mut db = mem_db(&damaged_records());
    assert!(db.raw_game(1).is_ok());
    for n in 2..=9 {
        assert!(db.raw_game(n).is_err(), "record {n} should be damaged");
    }
    assert!(db.raw_game(10).is_ok());
}

#[test]
fn damage_after_the_match_does_not_exclude_the_game() {
    let mut db = mem_db(&damaged_records());
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &pos_after("e2e4 e7e5"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    // Games 1-6 reach the position and move Nf3; the tail damage is ignored.
    assert_eq!(sans(&r), vec!["Nf3"]);
    assert_eq!(r.rows[0].total, 6);
    assert_eq!(r.total.as_ref().unwrap().total, 6);
    // The tree filter holds ply + 1 = 3 for exactly those games.
    let f = state.tree_filter().unwrap();
    assert_eq!(f.as_bytes()[..6], [3, 3, 3, 3, 3, 3]);
}

#[test]
fn damage_before_the_match_is_unreadable_and_outside_the_tree() {
    let mut db = mem_db(&damaged_records());
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &pos_after("e2e4 e7e5"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(r.unreadable_total, 3);
    assert_eq!(r.unreadable, vec![7, 8, 9]);
    let f = state.tree_filter().unwrap();
    assert_eq!(f.as_bytes(), &[3, 3, 3, 3, 3, 3, 0, 0, 0, 0]);
    // The intact game that never matches is neither counted nor unreadable.
    assert!(!r.unreadable.contains(&10));
}

#[test]
fn a_game_that_never_matches_because_of_an_error_is_unreadable_not_ignored() {
    // Target after 1.e4 e5 2.Nf3 Nc6 (ply 4). Record 3 breaks at ply 3 with
    // an illegal move, before it could match: unreadable. Record 1 matches.
    let mut db = mem_db(&damaged_records());
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &pos_after("e2e4 e7e5 g1f3 b8c6"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    // 1 matches (then the game ends: [end]); 2 and 3 break at the 4th move
    // before reaching it; 4-6 and 9 vary; see the exact sets below.
    let counted: Vec<u32> = state.tree_filter().unwrap().iter_games().collect();
    // Game 1 matches at ply 4 (value 5) with [end]. Games 2 (invalid code)
    // and 3 (illegal) fail before ply 4; games 4 and 5 cannot play the 4th
    // move; game 6 reaches ply 4 (comments are not read): [end].
    assert!(counted.contains(&1) && counted.contains(&6), "{counted:?}");
    assert!(r.unreadable.contains(&2) && r.unreadable.contains(&3));
    assert!(!counted.contains(&2) && !counted.contains(&3));
    assert_eq!(state.tree_filter().unwrap().get(1), 5);
}

#[test]
fn the_standard_start_needs_only_the_first_move_to_decode() {
    // Match at ply 0: the first move is read, nothing else (D957).
    let mut db = mem_db(&[
        truncated(&[E4]),                         // 1: cut after the first move
        stream(&[E4, 0x20, 0x11, END_GAME]),      // 2: garbage from the 2nd move on
        stream(&[0x20, END_GAME]),                // 3: the first move itself is invalid
        stream(&[E4, START_VARIATION, END_GAME]), // 4: variation never closes
    ]);
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(sans(&r), vec!["e4"]);
    assert_eq!(r.rows[0].total, 3);
    assert_eq!(r.unreadable, vec![3]);
    assert_eq!(state.tree_filter().unwrap().as_bytes(), &[1, 1, 0, 1]);
}

#[test]
fn damage_after_the_match_is_counted_in_the_best_games_filter_too() {
    // The tree filter is what `adjust` and `best_games` read.
    let mut db = mem_db(&damaged_records());
    let mut state = TreeState::default();
    let opts = TreeOptions {
        adjust: true,
        ..TreeOptions::default()
    };
    let r = run(&mut db, &pos_after("e2e4 e7e5"), &opts, &mut state).unwrap();
    let adjusted = r.adjusted_filter.unwrap();
    assert_eq!(adjusted.as_bytes(), &[3, 3, 3, 3, 3, 3, 0, 0, 0, 0]);
}

// ------------------------------------------------------------ D958 (a)

fn many_moves_fen() -> &'static str {
    // 218 legal moves for White.
    "R6R/3Q4/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp1Q4/kBNN1KB1 w - - 0 1"
}

fn many_moves_games(n: usize) -> Vec<base::B> {
    let pos = Position::from_fen(many_moves_fen()).unwrap();
    pos.legal_moves()
        .into_iter()
        .take(n)
        .map(|mv| {
            let mut game = Game::from_position(pos.clone());
            let root = game.root();
            game.add_move(root, mv).unwrap();
            base::B::of(game)
        })
        .collect()
}

fn search_with(
    db: &mut impl GameSource,
    pos: &Position,
    state: &mut TreeState,
    cache: &mut MemCache,
    token: &CancelToken,
    opts: &TreeOptions<'_>,
) -> Result<TreeResult, TreeError> {
    let filter = Filter::new(db.len());
    let query = TreeQuery {
        position: pos,
        ply: 0,
        main_filter: &filter,
    };
    search(db, &query, opts, state, cache, token, &mut NoProgress)
}

fn dump(state: &TreeState) -> String {
    format!("{state:?}")
}

/// A state filled by a successful search of a 4-game database, with a fast
/// copy (slow search) and a database filter.
fn populated_state() -> TreeState {
    let (_fx, mut db) = db_of(vec![gb("e2e4"), gb("e2e4"), gb("d2d4"), gb("g1f3")]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert!(state.tree_filter().is_some() && state.db_filter().is_some());
    state
}

#[test]
fn cancelled_search_keeps_a_stale_state_exactly_as_it_was() {
    let mut state = populated_state(); // 4 games
    let (_fx, mut other) = db_of(vec![gb("e2e4"), gb("d2d4"), gb("c2c4")]); // 3 games: stale
    let before = dump(&state);
    let token = CancelToken::new();
    token.cancel();
    let mut cache = MemCache::default();
    let err = search_with(
        &mut other,
        &Position::new(),
        &mut state,
        &mut cache,
        &token,
        &TreeOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::Cancelled), "{err:?}");
    assert_eq!(
        dump(&state),
        before,
        "stale state must not be dropped on failure"
    );
    assert_eq!(cache.stores, 0);
    assert_eq!(state.tree_filter().unwrap().len(), 4);
}

#[test]
fn too_many_moves_keeps_a_stale_state_exactly_as_it_was() {
    let mut state = populated_state(); // 4 games
    let (_fx, mut big) = db_of(many_moves_games(61)); // 61 games: stale + overflow
    let before = dump(&state);
    let mut cache = MemCache::default();
    let err = search_with(
        &mut big,
        &Position::from_fen(many_moves_fen()).unwrap(),
        &mut state,
        &mut cache,
        &CancelToken::new(),
        &TreeOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::TooManyMoves), "{err:?}");
    assert_eq!(dump(&state), before);
    assert_eq!(cache.stores, 0);
}

#[test]
fn too_many_moves_keeps_a_current_state_including_the_fast_copy() {
    let (_fx, mut big) = db_of(many_moves_games(61));
    let mut state = TreeState::default();
    // A successful search of the same database (no game reaches the standard
    // position, so the tree is empty) leaves a tree filter and a fast copy.
    run(
        &mut big,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let before = dump(&state);
    let mut cache = MemCache::default();
    let err = search_with(
        &mut big,
        &Position::from_fen(many_moves_fen()).unwrap(),
        &mut state,
        &mut cache,
        &CancelToken::new(),
        &TreeOptions {
            adjust: true,
            ..TreeOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::TooManyMoves), "{err:?}");
    assert_eq!(dump(&state), before);
    assert_eq!(cache.stores, 0);
}

// ------------------------------------------------------------ D958 (b)

#[test]
fn best_games_skips_an_unreadable_index_entry_instead_of_failing() {
    let line = "e2e4 e7e5 g1f3 b8c6";
    let list = vec![
        gb(line).elo(2000, 2000).done(),
        gb(line).elo(2900, 2900).done(), // entry 2 gets broken
        gb(line).elo(2500, 2500).done(),
        gb(line).elo(2100, 2100).done(),
    ];
    let fx = base::build(&list);
    let mut db = fx.open();
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let stats = PlayerStats::from_database(&db);
    drop(db);
    base::break_entry(&fx, 2);
    let mut db = fx.open();

    let mut ids = |order, max| -> Vec<u32> {
        let opts = BestOptions {
            max,
            order,
            ..BestOptions::default()
        };
        best_games(&mut db, &state, &stats, &opts)
            .unwrap()
            .iter()
            .map(|b| b.game)
            .collect()
    };
    // The best of the readable ones, by rating: 3 (2500), 4 (2100), 1 (2000).
    assert_eq!(ids(BestOrder::Best, 10), vec![3, 4, 1]);
    assert_eq!(
        ids(BestOrder::Best, 2),
        vec![3, 4],
        "skipped games take no slot"
    );
    // Database order, last ones: the unreadable 2 is passed over.
    assert_eq!(ids(BestOrder::Order, 10), vec![1, 3, 4]);
    assert_eq!(
        ids(BestOrder::Order, 3),
        vec![1, 3, 4],
        "skipped games take no slot"
    );
}

#[test]
fn best_games_does_not_fail_when_only_the_record_is_unreadable() {
    let line = "e2e4 e7e5 g1f3 b8c6";
    let list = vec![gb(line).done(), gb(line).done(), gb(line).done()];
    let fx = base::build(&list);
    let mut db = fx.open();
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let stats = PlayerStats::from_database(&db);
    drop(db);
    base::break_offset(&fx, 2);
    let mut db = fx.open();
    for order in [BestOrder::Best, BestOrder::Order] {
        let opts = BestOptions {
            max: 10,
            order,
            ..BestOptions::default()
        };
        let out = best_games(&mut db, &state, &stats, &opts).unwrap();
        let ids: Vec<u32> = out.iter().map(|b| b.game).collect();
        assert!(ids.contains(&1) && ids.contains(&3), "{order:?}: {ids:?}");
    }
}

// ------------------------------------------------------------ D958 (c)

fn db_with_unreadable_record() -> (base::Fixture, mychess_db::si4::Si4Database) {
    let list = vec![gb("e2e4").done(), gb("e2e4").done(), gb("e2e4").done()];
    let fx = base::build(&list);
    base::break_offset(&fx, 2);
    let db = fx.open();
    (fx, db)
}

#[test]
fn a_result_with_unreadable_games_is_not_stored_in_the_cache() {
    let (_fx, mut db) = db_with_unreadable_record();
    let mut state = TreeState::default();
    let mut cache = MemCache::default();
    let filter = Filter::new(3);
    let r = run_with(
        &mut db,
        &Position::new(),
        0,
        &filter,
        &TreeOptions::default(),
        &mut state,
        &mut cache,
    )
    .unwrap();
    assert_eq!(r.unreadable_total, 1);
    assert_eq!(cache.stores, 0);
    assert_eq!(cache.len(), 0);
    // The next search cannot be served from the cache: it scans again and
    // reports the unreadable game again.
    let r = run_with(
        &mut db,
        &Position::new(),
        0,
        &filter,
        &TreeOptions::default(),
        &mut state,
        &mut cache,
    )
    .unwrap();
    assert!(!r.from_cache);
    assert_eq!(r.unreadable, vec![2]);
    assert_eq!(cache.stores, 0);
}

#[test]
fn an_unreadable_index_entry_also_keeps_the_result_out_of_the_cache() {
    let list = vec![gb("e2e4").done(), gb("e2e4").done()];
    let fx = base::build(&list);
    base::break_entry(&fx, 1);
    let mut db = fx.open();
    let mut cache = MemCache::default();
    let r = run_with(
        &mut db,
        &Position::new(),
        0,
        &Filter::new(2),
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert_eq!(r.unreadable_total, 1);
    assert_eq!(cache.stores, 0);
}

#[test]
fn the_fast_mode_copy_is_kept_even_when_the_cache_is_not() {
    let (_fx, mut db) = db_with_unreadable_record();
    let mut state = TreeState::default();
    let mut cache = MemCache::default();
    let filter = Filter::new(3);
    let slow = run_with(
        &mut db,
        &Position::new(),
        0,
        &filter,
        &TreeOptions::default(),
        &mut state,
        &mut cache,
    )
    .unwrap();
    assert_eq!(slow.unreadable_total, 1);
    // The fast search examines only the games of the saved copy (1 and 3);
    // game 2 (value 0 in it) is not even read, so nothing is unreadable. A
    // fast search without a copy would be a full one and report game 2.
    let fast = run_with(
        &mut db,
        &Position::new(),
        0,
        &filter,
        &TreeOptions {
            fast: FastMode::Fast,
            ..TreeOptions::default()
        },
        &mut state,
        &mut cache,
    )
    .unwrap();
    assert!(!fast.from_cache);
    assert_eq!(fast.unreadable_total, 0, "the copy was used");
    assert_eq!(fast.searched, 2);
    assert_eq!(fast.rows[0].total, 2);
    assert_eq!(cache.stores, 0);
}

#[test]
fn a_clean_result_is_stored_in_the_cache_as_the_control_case() {
    let (_fx, mut db) = db_of(vec![gb("e2e4"), gb("e2e4")]);
    let mut cache = MemCache::default();
    run_with(
        &mut db,
        &Position::new(),
        0,
        &Filter::new(2),
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut cache,
    )
    .unwrap();
    assert_eq!(cache.stores, 1);
}

// ------------------------------------------------------------ D958 (d)
//
// `TreeError::UnsupportedPosition` has no black-box test: every way to build a
// `mychess_core::Position` (FEN with the 16-piece, one-king and no-back-rank-
// pawn checks of `fen.rs`, and legal play, which never adds pieces) already
// fits the SI4 `ListPosition` limits, so the error cannot be provoked through
// the public API. See the report.
