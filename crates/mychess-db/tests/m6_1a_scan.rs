//! Contract tests of the common scan of the advanced searches (spec
//! `docs/specs/m6-1-buscas-avancadas.md`, "Comuns às três buscas", D3201 to
//! D3207): filter operations, cancellation, unreadable games, blocks and
//! threads.
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::field_reassign_with_default,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_support.rs"]
mod support;

use mychess_core::Position;
use mychess_db::si4::IndexEntry;
use mychess_db::si4::RecordError;
use mychess_db::{
    BLOCK_GAMES, BoardMatch, BoardSearch, CancelToken, Filter, FilterOp, GameMatcher, NoProgress,
    SearchError, Shortcut, scan_games, search_board,
};
use pretty_assertions::assert_eq;
use support::{db_of, game, numbers, run_board, values};

/// Matches the games whose White name id is even, with value 7. The record is
/// never decoded.
struct EvenWhite;

impl GameMatcher for EvenWhite {
    fn prefilter(&self, _entry: &IndexEntry) -> Shortcut {
        Shortcut::Examine
    }
    fn evaluate(&self, entry: &IndexEntry, _record: &[u8]) -> Result<Option<u8>, RecordError> {
        Ok(entry.white_id.is_multiple_of(2).then_some(7))
    }
}

/// Four games with distinct White names, ids 0 to 3 in the name base.
fn four() -> (base::Fixture, mychess_db::si4::Si4Database) {
    let games = (0..4)
        .map(|i| {
            let mut g = game("e2e4");
            g.tags_mut().white = format!("White {i}");
            g
        })
        .collect();
    db_of(games)
}

fn scan(
    db: &mut mychess_db::si4::Si4Database,
    op: FilterOp,
    filter: &mut Filter,
) -> mychess_db::SearchReport {
    scan_games(
        db,
        &EvenWhite,
        op,
        filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

#[test]
fn reset_examines_every_game_and_ignores_the_old_filter() {
    let (_fx, mut db) = four();
    let mut filter = Filter::new(4);
    filter.fill(0);
    let report = scan(&mut db, FilterOp::Reset, &mut filter);
    let ids: Vec<u32> = (1..=4).map(|n| db.entry(n).unwrap().white_id % 2).collect();
    let expected: Vec<(u32, u8)> = ids
        .iter()
        .enumerate()
        .filter(|(_, id)| **id == 0)
        .map(|(i, _)| (i as u32 + 1, 7))
        .collect();
    assert_eq!(values(&filter), expected);
    assert_eq!(report.start_count, 4);
    assert_eq!(report.matched, expected.len() as u32);
    assert!(!report.interrupted);
}

#[test]
fn and_examines_only_games_in_the_filter() {
    let (_fx, mut db) = four();
    let mut filter = Filter::new(4);
    filter.fill(0);
    filter.set(1, 3);
    filter.set(2, 3);
    let report = scan(&mut db, FilterOp::And, &mut filter);
    // Games 3 and 4 were out and stay out, whatever the matcher says.
    assert!(!filter.contains(3) && !filter.contains(4));
    assert_eq!(report.start_count, 2);
    for n in [1, 2] {
        let even = db.entry(n).unwrap().white_id % 2 == 0;
        assert_eq!(filter.get(n), if even { 7 } else { 0 }, "game {n}");
    }
}

#[test]
fn or_keeps_the_games_already_in_and_does_not_remove_others() {
    let (_fx, mut db) = four();
    let mut filter = Filter::new(4);
    filter.fill(0);
    filter.set(1, 3);
    let even: Vec<bool> = (1..=4)
        .map(|n| db.entry(n).unwrap().white_id % 2 == 0)
        .collect();
    let report = scan(&mut db, FilterOp::Or, &mut filter);
    // Game 1 keeps its value 3 even if the matcher would say no or 7.
    assert_eq!(filter.get(1), 3);
    for n in 2..=4u32 {
        assert_eq!(
            filter.get(n),
            if even[n as usize - 1] { 7 } else { 0 },
            "game {n}"
        );
    }
    assert_eq!(report.start_count, 4);
}

#[test]
fn games_without_a_record_leave_the_filter() {
    let (fx, db) = four();
    drop(db);
    base::zero_length(&fx, 2);
    let mut db = fx.open();
    let mut filter = Filter::new(4);
    scan(&mut db, FilterOp::Reset, &mut filter);
    assert!(!filter.contains(2));
}

#[test]
fn unreadable_games_do_not_match_and_are_reported() {
    let (fx, db) = four();
    drop(db);
    base::break_entry(&fx, 3);
    base::break_offset(&fx, 4);
    let mut db = fx.open();
    let mut filter = Filter::new(4);
    let report = scan(&mut db, FilterOp::Reset, &mut filter);
    assert!(!filter.contains(3) && !filter.contains(4));
    assert_eq!(report.unreadable, vec![3, 4]);
    assert_eq!(report.unreadable_total, 2);
}

#[test]
fn a_wrong_filter_size_is_an_error_and_nothing_changes() {
    let (_fx, mut db) = four();
    let mut filter = Filter::new(3);
    let err = scan_games(
        &mut db,
        &EvenWhite,
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert_eq!(
        err,
        SearchError::FilterSize {
            filter: 3,
            games: 4
        }
    );
    assert_eq!(filter, Filter::new(3));
}

#[test]
fn a_token_cancelled_before_the_scan_leaves_the_filter_alone() {
    let (_fx, mut db) = four();
    let mut filter = Filter::new(4);
    filter.set(2, 9);
    let before = filter.clone();
    let token = CancelToken::new();
    token.cancel();
    let report = scan_games(
        &mut db,
        &EvenWhite,
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &token,
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!(filter, before);
    assert_eq!(report.matched, before.count());
}

/// Cancels the token while evaluating one game.
struct CancelsAt {
    token: CancelToken,
    white_id: u32,
}

impl GameMatcher for CancelsAt {
    fn prefilter(&self, _entry: &IndexEntry) -> Shortcut {
        Shortcut::Examine
    }
    fn evaluate(&self, entry: &IndexEntry, _record: &[u8]) -> Result<Option<u8>, RecordError> {
        if entry.white_id == self.white_id {
            self.token.cancel();
        }
        Ok(Some(5))
    }
}

#[test]
fn cancelling_in_the_middle_keeps_the_old_filter_byte_for_byte() {
    let games = (0..50)
        .map(|i| {
            let mut g = game("e2e4 e7e5");
            g.tags_mut().white = format!("Player {i:02}");
            g
        })
        .collect();
    let (_fx, mut db) = db_of(games);
    let mut filter = Filter::new(50);
    for n in (1..=50).step_by(3) {
        filter.set(n, 2);
    }
    let before = filter.clone();
    let token = CancelToken::new();
    let matcher = CancelsAt {
        token: token.clone(),
        white_id: db.entry(25).unwrap().white_id,
    };
    let report = scan_games(
        &mut db,
        &matcher,
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &token,
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!(filter.as_bytes(), before.as_bytes());
}

#[test]
fn progress_ends_with_all_the_games() {
    let (_fx, mut db) = four();
    let mut filter = Filter::new(4);
    let mut seen = Vec::new();
    {
        let mut sink = |done, total| seen.push((done, total));
        scan_games(
            &mut db,
            &EvenWhite,
            FilterOp::Reset,
            &mut filter,
            &mut sink,
            &CancelToken::new(),
        )
        .unwrap();
    }
    assert_eq!(seen.last(), Some(&(4, 4)));
}

/// A database that crosses the block size: the result is the same with one
/// and with several rayon threads.
#[test]
fn many_blocks_give_the_same_filter_with_one_and_many_threads() {
    let total = BLOCK_GAMES * 2 + 37;
    let games = (0..total)
        .map(|i| {
            if i % 3 == 0 {
                game("e2e4 e7e5")
            } else if i % 3 == 1 {
                game("d2d4 d7d5 c2c4")
            } else {
                game("e2e4 c7c5")
            }
        })
        .collect();
    let (_fx, mut db) = db_of(games);
    let query = BoardSearch {
        kind: BoardMatch::Exact,
        ..BoardSearch::new(support::pos_after("e2e4 e7e5"))
    };
    let mut run = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_board(&mut db, &query).0)
    };
    let one = run(1);
    let many = run(4);
    assert_eq!(one.as_bytes(), many.as_bytes());
    assert_eq!(one.count() as usize, total.div_ceil(3));
    // Value: ply 2, plus 1.
    assert_eq!(one.get(1), 3);
    assert_eq!(one.get(2), 0);
}

#[test]
fn the_start_count_of_an_and_search_is_the_filter_count() {
    let (_fx, mut db) = four();
    let mut filter = Filter::new(4);
    filter.fill(0);
    filter.set(1, 1);
    filter.set(4, 1);
    let query = BoardSearch {
        op: FilterOp::And,
        ..BoardSearch::new(Position::new())
    };
    let report = search_board(
        &mut db,
        &query,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(report.start_count, 2);
    assert_eq!(numbers(&filter), vec![1, 4]);
}
