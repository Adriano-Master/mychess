//! Building a book from games (D1112, D1118).

#![allow(clippy::assert_is_empty)]

mod common;

use common::{code, is_sorted_by_key, line, mv, pos_after};
use mychess_book::{Book, BookBuilder, BookError, BuildOptions, polyglot_key};
use mychess_core::{GameResult, Move, Position};

fn opts() -> BuildOptions {
    BuildOptions {
        min_game: 1,
        ..BuildOptions::default()
    }
}

/// `(uci, weight)` of the book moves of `pos`, in file order.
fn listed(book: &Book, pos: &Position) -> Vec<(String, u16)> {
    book.moves(pos)
        .iter()
        .map(|m| (m.mv.to_uci(pos.castling_mode()), m.weight))
        .collect()
}

fn add(b: &mut BookBuilder, ucis: &[&str], result: GameResult) {
    b.add_game(line(ucis), result).expect("legal game");
}

fn repeat(b: &mut BookBuilder, n: usize, ucis: &[&str], result: GameResult) {
    for _ in 0..n {
        add(b, ucis, result);
    }
}

#[test]
fn default_options_follow_book_make() {
    let o = BuildOptions::default();
    assert_eq!(o.max_ply, 1024);
    assert_eq!(o.min_game, 3);
    assert!(o.min_score.abs() < f64::EPSILON);
    assert!(!o.only_white && !o.only_black && !o.uniform);
}

#[test]
fn weights_follow_the_result_from_the_point_of_view_of_the_mover() {
    let mut b = BookBuilder::new(opts());
    add(&mut b, &["e2e4", "e7e5"], GameResult::White);
    add(&mut b, &["e2e4", "c7c5"], GameResult::Draw);
    add(&mut b, &["d2d4"], GameResult::Black);
    let (book, report) = b.finish();
    let start = Position::new();
    // e4: win (2) + draw (1) = 3; d4: loss = 0 -> dropped.
    assert_eq!(listed(&book, &start), [("e2e4".to_string(), 3)]);
    // Black after e4: e5 lost (0, dropped), c5 drew (1).
    let after = pos_after(&["e2e4"]);
    assert_eq!(listed(&book, &after), [("c7c5".to_string(), 1)]);
    assert_eq!(report.games_seen, 3);
    assert_eq!(report.games_used, 3);
    assert_eq!(report.games_illegal, 0);
}

#[test]
fn a_black_victory_rewards_black_moves_and_punishes_white_moves() {
    let mut b = BookBuilder::new(opts());
    add(&mut b, &["e2e4", "e7e5", "g1f3"], GameResult::Black);
    let (book, _) = b.finish();
    assert!(book.moves(&Position::new()).is_empty(), "white lost");
    assert_eq!(
        listed(&book, &pos_after(&["e2e4"])),
        [("e7e5".to_string(), 2)]
    );
    assert!(book.moves(&pos_after(&["e2e4", "e7e5"])).is_empty());
}

#[test]
fn unknown_result_counts_as_a_draw() {
    let mut b = BookBuilder::new(opts());
    add(&mut b, &["e2e4", "e7e5"], GameResult::Unknown);
    let (book, _) = b.finish();
    assert_eq!(listed(&book, &Position::new()), [("e2e4".to_string(), 1)]);
    assert_eq!(
        listed(&book, &pos_after(&["e2e4"])),
        [("e7e5".to_string(), 1)]
    );
}

#[test]
fn n_and_sum_are_written_as_zero() {
    let mut b = BookBuilder::new(opts());
    repeat(&mut b, 5, &["e2e4", "e7e5", "g1f3"], GameResult::White);
    let (book, _) = b.finish();
    assert!(!book.is_empty());
    assert!(book.entries().iter().all(|e| e.learn == 0));
}

#[test]
fn min_game_filters_by_the_number_of_games_of_each_move() {
    let build = |min_game| {
        let mut b = BookBuilder::new(BuildOptions {
            min_game,
            ..BuildOptions::default()
        });
        repeat(&mut b, 3, &["e2e4"], GameResult::White);
        repeat(&mut b, 2, &["d2d4"], GameResult::White);
        b.finish().0
    };
    let start = Position::new();
    assert_eq!(listed(&build(3), &start), [("e2e4".to_string(), 6)]);
    assert_eq!(build(2).moves(&start).len(), 2);
    assert_eq!(build(1).moves(&start).len(), 2);
    assert!(build(4).is_empty());
}

#[test]
fn min_score_keeps_moves_whose_score_is_at_least_the_threshold() {
    // e4 four times: W, W, D, L -> sum 5, n 4 -> 62.5 %. d4: always wins.
    let build = |min_score: f64| {
        let mut b = BookBuilder::new(BuildOptions {
            min_game: 1,
            min_score,
            ..BuildOptions::default()
        });
        add(&mut b, &["e2e4"], GameResult::White);
        add(&mut b, &["e2e4"], GameResult::White);
        add(&mut b, &["e2e4"], GameResult::Draw);
        add(&mut b, &["e2e4"], GameResult::Black);
        add(&mut b, &["d2d4"], GameResult::White);
        b.finish().0
    };
    let start = Position::new();
    let names = |b: &Book| -> Vec<String> { listed(b, &start).into_iter().map(|x| x.0).collect() };
    assert_eq!(build(0.0).moves(&start).len(), 2);
    assert_eq!(names(&build(62.4)).len(), 2);
    assert_eq!(names(&build(62.6)), ["d2d4"]);
    assert_eq!(names(&build(100.0)), ["d2d4"]);
    // Out of range values are clamped, not an error.
    assert_eq!(names(&build(250.0)), ["d2d4"]);
    assert_eq!(build(-30.0).moves(&start).len(), 2);
    let nan = build(f64::NAN);
    assert!(nan.moves(&start).len() <= 2);
}

#[test]
fn moves_that_never_scored_are_dropped_whatever_the_options() {
    let mut b = BookBuilder::new(BuildOptions {
        min_game: 1,
        uniform: true,
        ..BuildOptions::default()
    });
    add(&mut b, &["e2e4"], GameResult::Black);
    add(&mut b, &["d2d4"], GameResult::White);
    let (book, _) = b.finish();
    assert_eq!(listed(&book, &Position::new()), [("d2d4".to_string(), 1)]);
}

#[test]
fn only_white_keeps_white_moves_and_only_black_keeps_black_moves() {
    let make = |only_white, only_black| {
        let mut b = BookBuilder::new(BuildOptions {
            min_game: 1,
            only_white,
            only_black,
            ..BuildOptions::default()
        });
        add(&mut b, &["e2e4", "c7c5", "g1f3"], GameResult::Draw);
        b.finish()
    };
    let colour_of_entries = |book: &Book| -> Vec<bool> {
        let positions = [
            Position::new(),
            pos_after(&["e2e4"]),
            pos_after(&["e2e4", "c7c5"]),
        ];
        positions
            .iter()
            .map(|p| !book.moves(p).is_empty())
            .collect()
    };
    let (all, _) = make(false, false);
    assert_eq!(colour_of_entries(&all), [true, true, true]);
    let (white, _) = make(true, false);
    assert_eq!(colour_of_entries(&white), [true, false, true]);
    let (black, _) = make(false, true);
    assert_eq!(colour_of_entries(&black), [false, true, false]);
    let (neither, report) = make(true, true);
    assert!(neither.is_empty(), "both flags discard everything");
    assert_eq!(report.entries_kept, 0);
}

#[test]
fn uniform_gives_every_kept_move_weight_one() {
    let mut b = BookBuilder::new(BuildOptions {
        min_game: 1,
        uniform: true,
        ..BuildOptions::default()
    });
    repeat(&mut b, 7, &["e2e4"], GameResult::White);
    repeat(&mut b, 2, &["d2d4"], GameResult::Draw);
    let (book, _) = b.finish();
    assert_eq!(
        listed(&book, &Position::new()),
        [("e2e4".to_string(), 1), ("d2d4".to_string(), 1)]
    );
}

#[test]
fn max_ply_cuts_the_game_and_later_moves_are_not_even_validated() {
    let mut b = BookBuilder::new(BuildOptions {
        min_game: 1,
        max_ply: 2,
        ..BuildOptions::default()
    });
    let start = Position::new();
    let mut moves = line(&["e2e4", "e7e5"]);
    // The third move is illegal (e2e4 again) but lies beyond max_ply.
    moves.push(mv(&start, "e2e4"));
    b.add_game(moves, GameResult::White)
        .expect("cut before the bad move");
    let (book, report) = b.finish();
    assert_eq!(report.games_illegal, 0);
    assert_eq!(report.games_used, 1);
    assert_eq!(book.len(), 1, "only e4 (white won); e5 lost");
    let long = {
        let mut b = BookBuilder::new(BuildOptions {
            min_game: 1,
            max_ply: 0,
            ..BuildOptions::default()
        });
        add(&mut b, &["e2e4"], GameResult::White);
        b.finish().0
    };
    assert!(long.is_empty());
}

#[test]
fn an_illegal_move_discards_the_game_without_partial_statistics() {
    let start = Position::new();
    let mut b = BookBuilder::new(opts());
    // 1.e4 e5 then e2e4 again: illegal at ply 2.
    let mut bad = line(&["e2e4", "e7e5"]);
    bad.push(mv(&start, "e2e4"));
    let err = b.add_game(bad, GameResult::White).expect_err("illegal");
    assert!(matches!(err, BookError::IllegalMove { ply: 2 }), "{err:?}");
    let (empty, report) = b.finish();
    assert!(empty.is_empty(), "plies 0 and 1 must not leave statistics");
    assert_eq!(report.games_illegal, 1);
    assert_eq!(report.games_used, 0);
    assert_eq!(report.games_seen, 1);
    assert_eq!(report.entries_before_filter, 0);
}

#[test]
fn an_illegal_game_does_not_stop_later_games() {
    let mut b = BookBuilder::new(opts());
    assert!(b.add_game([Move::Null], GameResult::White).is_err());
    let after = pos_after(&["e2e4"]);
    assert!(
        b.add_game(vec![mv(&after, "e7e5")], GameResult::White)
            .is_err()
    );
    add(&mut b, &["d2d4"], GameResult::White);
    let (book, report) = b.finish();
    assert_eq!(listed(&book, &Position::new()), [("d2d4".to_string(), 2)]);
    assert_eq!(report.games_illegal, 2);
    assert_eq!(report.games_used, 1);
    assert_eq!(report.games_seen, 3);
}

#[test]
fn games_from_a_non_standard_start_are_discarded_and_counted() {
    let mut b = BookBuilder::new(opts());
    let custom = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").expect("fen");
    let m = mv(&custom, "e2e4");
    let outcome = b.add_game_from(&custom, [m], GameResult::White);
    drop(outcome);
    let (book, report) = b.finish();
    assert!(book.is_empty());
    assert_eq!(report.games_non_standard_start, 1);
    assert_eq!(report.games_used, 0);
    assert_eq!(report.games_illegal, 0);
}

#[test]
fn add_game_from_the_standard_start_behaves_like_add_game() {
    let mut b = BookBuilder::new(opts());
    b.add_game_from(&Position::new(), line(&["e2e4"]), GameResult::White)
        .expect("standard start");
    let (book, report) = b.finish();
    assert_eq!(listed(&book, &Position::new()), [("e2e4".to_string(), 2)]);
    assert_eq!(report.games_used, 1);
    assert_eq!(report.games_non_standard_start, 0);
}

#[test]
fn transpositions_accumulate_on_the_same_entry() {
    let mut b = BookBuilder::new(BuildOptions {
        min_game: 2,
        ..BuildOptions::default()
    });
    add(
        &mut b,
        &["g1f3", "g8f6", "b1c3", "b8c6", "e2e4"],
        GameResult::White,
    );
    add(
        &mut b,
        &["b1c3", "g8f6", "g1f3", "b8c6", "e2e4"],
        GameResult::White,
    );
    let (book, _) = b.finish();
    let pos = pos_after(&["g1f3", "g8f6", "b1c3", "b8c6"]);
    assert_eq!(listed(&book, &pos), [("e2e4".to_string(), 4)]);
}

#[test]
fn output_is_sorted_by_key_then_by_descending_weight() {
    let mut b = BookBuilder::new(opts());
    repeat(&mut b, 3, &["e2e4", "e7e5", "g1f3"], GameResult::White);
    repeat(&mut b, 1, &["d2d4", "d7d5"], GameResult::White);
    repeat(&mut b, 2, &["c2c4"], GameResult::White);
    repeat(&mut b, 1, &["e2e4", "c7c5"], GameResult::Black);
    let (book, report) = b.finish();
    assert!(is_sorted_by_key(&book));
    assert!(book.was_sorted());
    // Weights at the start: e4 = 3 * 2 = 6, c4 = 4, d4 = 2.
    let start_entries = book.entries_for(&Position::new());
    let weights: Vec<u16> = start_entries.iter().map(|e| e.weight).collect();
    assert_eq!(weights, [6, 4, 2]);
    assert_eq!(start_entries[0].mv, code(&Position::new(), "e2e4"));
    for w in book.entries().windows(2) {
        if w[0].key == w[1].key {
            assert!(w[0].weight >= w[1].weight);
        }
    }
    assert_eq!(report.entries_kept, book.len());
    assert!(report.entries_before_filter >= report.entries_kept);
}

#[test]
fn equal_weights_keep_the_order_in_which_moves_first_appeared() {
    let mut b = BookBuilder::new(opts());
    add(&mut b, &["d2d4"], GameResult::White);
    add(&mut b, &["e2e4"], GameResult::White);
    add(&mut b, &["c2c4"], GameResult::White);
    let (book, _) = b.finish();
    let names: Vec<String> = listed(&book, &Position::new())
        .into_iter()
        .map(|x| x.0)
        .collect();
    assert_eq!(names, ["d2d4", "e2e4", "c2c4"]);
}

#[test]
fn report_counts_distinct_move_entries_before_and_after_filtering() {
    let mut b = BookBuilder::new(BuildOptions {
        min_game: 2,
        ..BuildOptions::default()
    });
    repeat(&mut b, 2, &["e2e4", "e7e5"], GameResult::White);
    add(&mut b, &["d2d4"], GameResult::White);
    let (_, report) = b.finish();
    // Distinct (position, move) pairs: e4, e5, d4.
    assert_eq!(report.entries_before_filter, 3);
    // e4 (n=2, sum 4) stays; e5 (loss) dropped; d4 has n=1.
    assert_eq!(report.entries_kept, 1);
}

#[test]
fn halving_at_16384_games_divides_n_and_sum_of_every_move_of_the_key() {
    let mut b = BookBuilder::new(BuildOptions {
        min_game: 1,
        ..BuildOptions::default()
    });
    // d4 first: 11 draws -> n 11, sum 11.
    repeat(&mut b, 11, &["d2d4"], GameResult::Draw);
    // e4: 16384 wins. On the last one n reaches 16384 and the key is halved:
    // e4 -> n 8192, sum 16384; d4 -> n 6, sum 6 ((11 + 1) / 2).
    repeat(&mut b, 16384, &["e2e4"], GameResult::White);
    let (book, _) = b.finish();
    let got = listed(&book, &Position::new());
    assert_eq!(
        got,
        [("e2e4".to_string(), 16384), ("d2d4".to_string(), 6)],
        "without halving e4 would weigh 32768 and d4 11"
    );
}

#[test]
fn counting_continues_after_a_halving() {
    let mut b = BookBuilder::new(BuildOptions {
        min_game: 1,
        ..BuildOptions::default()
    });
    repeat(&mut b, 16384 + 100, &["e2e4"], GameResult::White);
    let (book, _) = b.finish();
    // After halving: sum 16384; 100 more wins add 200.
    assert_eq!(
        listed(&book, &Position::new()),
        [("e2e4".to_string(), 16584)]
    );
}

#[test]
fn min_game_sees_the_halved_count() {
    let make = |min_game| {
        let mut b = BookBuilder::new(BuildOptions {
            min_game,
            ..BuildOptions::default()
        });
        repeat(&mut b, 11, &["d2d4"], GameResult::Draw);
        repeat(&mut b, 16384, &["e2e4"], GameResult::White);
        b.finish().0
    };
    let start = Position::new();
    // d4 has n = 6 after the halving.
    assert_eq!(make(6).moves(&start).len(), 2);
    assert_eq!(make(7).moves(&start).len(), 1);
}

#[test]
fn built_books_save_and_reopen_unchanged() {
    let mut b = BookBuilder::new(opts());
    repeat(
        &mut b,
        3,
        &["e2e4", "e7e5", "g1f3", "b8c6"],
        GameResult::White,
    );
    repeat(&mut b, 2, &["d2d4", "g8f6"], GameResult::Draw);
    let (mut book, _) = b.finish();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("built.bin");
    book.save_as(&path).expect("save");
    let reopened = Book::open(&path).expect("open");
    assert_eq!(reopened.entries(), book.entries());
    assert!(reopened.validate().is_clean());
    let key = polyglot_key(&Position::new());
    assert!(reopened.entries().iter().any(|e| e.key == key));
}

#[test]
fn an_empty_builder_gives_an_empty_book() {
    let (book, report) = BookBuilder::new(BuildOptions::default()).finish();
    assert!(book.is_empty());
    assert_eq!(report.games_seen, 0);
    assert_eq!(report.entries_kept, 0);
}

#[test]
fn a_game_with_no_moves_counts_as_used_and_adds_nothing() {
    let mut b = BookBuilder::new(opts());
    b.add_game(Vec::new(), GameResult::Draw)
        .expect("empty game");
    let (book, report) = b.finish();
    assert!(book.is_empty());
    assert_eq!(report.games_seen, 1);
}
