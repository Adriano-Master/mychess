//! Unit tests of the scan loop of the piece tracker: the cut by blocks and
//! the cancellation in the middle (the public tests cover the rules).

use std::path::Path;

use mychess_core::Square;

use super::*;
use crate::pgn_db::{OpenOptions, PgnDatabase};
use crate::progress::NoProgress;

fn database(games: usize) -> PgnDatabase {
    let one = "[Event \"E\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
               1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0\n\n";
    let text = one.repeat(games);
    PgnDatabase::from_reader(
        text.as_bytes(),
        Path::new("fixture.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("fixture opens")
    .0
}

fn query() -> PieceTrackQuery {
    PieceTrackQuery {
        squares: vec!["f1".parse::<Square>().expect("square")],
        start_move: 1,
        end_move: 20,
        mode: TrackMode::Games,
    }
}

fn filter_of(db: &PgnDatabase) -> Filter {
    let mut filter = Filter::new(db.len());
    filter.fill(1);
    filter
}

#[test]
fn every_block_size_gives_the_same_result() {
    let mut db = database(23);
    let filter = filter_of(&db);
    let reference = piece_track(
        &mut db,
        &filter,
        &query(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("scan");
    assert_eq!(reference.games_used, 23);
    assert_eq!(reference.counts[Square::new(33).to_usize()], 23); // b5
    for block in [1, 2, 5, 22, 23, 24, 1000] {
        let r = piece_track_with(
            &mut db,
            &filter,
            &query(),
            &mut NoProgress,
            &CancelToken::new(),
            block,
            1,
        )
        .expect("scan");
        assert_eq!(r, reference, "block {block}");
    }
}

#[test]
fn cancelling_at_the_fourth_report_counts_the_three_games_read() {
    let mut db = database(10);
    let filter = filter_of(&db);
    let cancel = CancelToken::new();
    let handle = cancel.clone();
    let mut calls = 0u32;
    // The ticker fires at every game: the fourth report is made before game
    // 4 is read, with games 1 to 3 waiting in the block.
    let mut sink = move |_: u64, _: u64| {
        calls += 1;
        if calls == 4 {
            handle.cancel();
        }
    };
    let r = piece_track_with(&mut db, &filter, &query(), &mut sink, &cancel, 2, 1).expect("scan");
    assert!(r.interrupted);
    assert_eq!(r.games_used, 3);
    assert_eq!(r.games, 10);
    assert_eq!(r.counts[Square::new(33).to_usize()], 3);
}

#[test]
fn progress_ends_at_the_total_when_not_cancelled() {
    let mut db = database(7);
    let filter = filter_of(&db);
    let mut last = (0, 0);
    let mut sink = |done: u64, total: u64| last = (done, total);
    let r = piece_track(&mut db, &filter, &query(), &mut sink, &CancelToken::new()).expect("scan");
    assert!(!r.interrupted);
    assert_eq!(last, (7, 7));
}
