//! Opening, listing, "leading to book", picking and validating (D1101-D1107,
//! D1113, D1119).

#![allow(clippy::cast_possible_truncation, clippy::assert_is_empty)]

mod common;

use std::cell::Cell;

use common::{book_with, code, entry, mv, pos_after};
use mychess_book::{Book, BookError, Entry, MAX_FILE_SIZE, polyglot_key};
use mychess_core::Position;

fn write_book(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn bytes_of(entries: &[Entry]) -> Vec<u8> {
    Book::from_entries(entries.to_vec()).to_bytes()
}

// ---------------------------------------------------------------- open

#[test]
fn open_empty_file_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_book(dir.path(), "empty.bin", &[]);
    assert!(Book::open(&path).is_err());
}

#[test]
fn open_file_shorter_than_one_record_is_too_short() {
    let dir = tempfile::tempdir().expect("tempdir");
    for len in [1usize, 8, 15] {
        let path = write_book(dir.path(), "short.bin", &vec![0u8; len]);
        assert!(
            matches!(Book::open(&path), Err(BookError::TooShort { len: l }) if l == len as u64),
            "len {len}"
        );
    }
}

#[test]
fn open_missing_file_is_an_io_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err = Book::open(dir.path().join("nope.bin")).expect_err("missing");
    assert!(matches!(err, BookError::Io { .. }), "{err:?}");
}

#[test]
fn open_exactly_one_record_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_book(dir.path(), "one.bin", &bytes_of(&[entry(7, 1, 2)]));
    let book = Book::open(&path).expect("open");
    assert_eq!(book.len(), 1);
    assert_eq!(book.path(), Some(path.as_path()));
}

#[test]
fn trailing_bytes_that_are_not_a_whole_record_are_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut bytes = bytes_of(&[entry(1, 1, 1), entry(2, 1, 1), entry(3, 1, 1)]);
    bytes.extend_from_slice(&[0xaa; 15]);
    let path = write_book(dir.path(), "tail.bin", &bytes);
    let book = Book::open(&path).expect("open");
    assert_eq!(book.len(), 3);
    assert_eq!(book.validate().trailing_bytes, 15);
    assert!(!book.validate().is_clean());
}

#[test]
fn unordered_file_opens_sorted_in_memory_and_keeps_learn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mk = |key, mv, learn| Entry {
        key,
        mv,
        weight: 5,
        learn,
    };
    let raw = [
        mk(9, 1, 0x0001_0002),
        mk(2, 10, 0xdead_beef),
        mk(5, 1, 7),
        mk(2, 11, 0x1234_5678),
    ];
    // Write the records exactly in this (wrong) order.
    let mut bytes = Vec::new();
    for e in &raw {
        bytes.extend_from_slice(&e.key.to_be_bytes());
        bytes.extend_from_slice(&e.mv.to_be_bytes());
        bytes.extend_from_slice(&e.weight.to_be_bytes());
        bytes.extend_from_slice(&e.learn.to_be_bytes());
    }
    let path = write_book(dir.path(), "unsorted.bin", &bytes);
    let book = Book::open(&path).expect("open");
    assert!(!book.was_sorted());
    assert!(!book.validate().was_sorted);
    // Stable: the two key-2 records keep their relative order, learn intact.
    assert_eq!(book.entries(), [raw[1], raw[3], raw[2], raw[0]]);
    // The original file is not touched by opening.
    assert_eq!(std::fs::read(&path).expect("read"), bytes);
}

#[test]
fn unordered_book_still_answers_queries_and_saves_sorted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let start = Position::new();
    let k = polyglot_key(&start);
    // High-bit key first: a signed comparison would call this sorted.
    let after = pos_after(&["e2e4"]);
    let entries = vec![
        entry(polyglot_key(&after), code(&after, "e7e5"), 4),
        entry(k, code(&start, "e2e4"), 9),
    ];
    let mut bytes = Vec::new();
    for e in &entries {
        bytes.extend_from_slice(&e.key.to_be_bytes());
        bytes.extend_from_slice(&e.mv.to_be_bytes());
        bytes.extend_from_slice(&e.weight.to_be_bytes());
        bytes.extend_from_slice(&e.learn.to_be_bytes());
    }
    let path = write_book(dir.path(), "u.bin", &bytes);
    let mut book = Book::open(&path).expect("open");
    // 0x823c... > 0x463b... as unsigned: the file is out of order.
    assert!(!book.was_sorted());
    assert_eq!(book.moves(&start).len(), 1);
    assert_eq!(book.moves(&after).len(), 1);
    book.save().expect("save");
    let reopened = Book::open(&path).expect("reopen");
    assert!(reopened.was_sorted());
    assert_eq!(reopened.entries(), book.entries());
    assert!(reopened.entries().windows(2).all(|w| w[0].key <= w[1].key));
}

#[test]
fn keys_are_ordered_as_unsigned_numbers() {
    let book = Book::from_entries(vec![
        entry(0, 1, 1),
        entry(1, 1, 1),
        entry(0x8000_0000_0000_0000, 1, 1),
        entry(u64::MAX, 1, 1),
    ]);
    assert!(book.was_sorted(), "signed ordering would flag this book");
}

#[cfg(unix)]
#[test]
fn file_above_the_size_limit_is_rejected_without_reading_it() {
    // A sparse file one byte over the limit: only metadata is needed.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("huge.bin");
    let file = std::fs::File::create(&path).expect("create");
    file.set_len(MAX_FILE_SIZE + 1).expect("sparse");
    drop(file);
    match Book::open(&path) {
        Err(BookError::TooLarge { size, limit }) => {
            assert_eq!(size, MAX_FILE_SIZE + 1);
            assert_eq!(limit, MAX_FILE_SIZE);
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }
    assert_eq!(MAX_FILE_SIZE, 128 * 1024 * 1024);
}

#[test]
fn read_only_file_opens_read_only_and_refuses_edits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_book(dir.path(), "ro.bin", &bytes_of(&[entry(1, 1, 1)]));
    let mut perms = std::fs::metadata(&path).expect("meta").permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).expect("chmod");
    let opened = Book::open(&path);
    #[allow(clippy::permissions_set_readonly_false)]
    {
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).expect("restore");
    }
    let mut book = opened.expect("open");
    assert!(book.is_read_only());
    assert!(matches!(book.save(), Err(BookError::ReadOnly)));
}

#[test]
fn open_readonly_flags_the_book_even_if_the_file_is_writable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_book(dir.path(), "w.bin", &bytes_of(&[entry(1, 1, 1)]));
    assert!(!Book::open(&path).expect("open").is_read_only());
    let mut ro = Book::open_readonly(&path).expect("open");
    assert!(ro.is_read_only());
    assert!(matches!(ro.save(), Err(BookError::ReadOnly)));
}

// --------------------------------------------------------------- moves

#[test]
fn moves_keep_only_positive_nonzero_legal_entries_in_file_order() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let book = Book::from_entries(vec![
        entry(k, code(&pos, "d2d4"), 10),
        entry(k, 0, 5),                  // absent move
        entry(k, 0x0abc, 5),             // illegal in the position
        entry(k, code(&pos, "c2c4"), 0), // weight zero
        entry(k, code(&pos, "e2e4"), 30),
    ]);
    let moves = book.moves(&pos);
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0].mv, mv(&pos, "d2d4"));
    assert_eq!(moves[1].mv, mv(&pos, "e2e4"));
    assert_eq!((moves[0].weight, moves[1].weight), (10, 30));
    // Sum of ALL entries of the key: 10 + 5 + 5 + 0 + 30 = 50.
    assert_eq!((moves[0].percent, moves[1].percent), (20, 60));
    assert_eq!(moves[1].code, code(&pos, "e2e4"));
    // Weight-ordered view is separate and stable.
    let sorted = book.moves_sorted_by_weight(&pos);
    assert_eq!(sorted[0].mv, mv(&pos, "e2e4"));
}

#[test]
fn moves_of_an_absent_position_are_empty() {
    let pos = Position::new();
    let other = pos_after(&["e2e4"]);
    let book = book_with(&other, &[("e7e5", 3)]);
    assert!(book.moves(&pos).is_empty());
    assert!(Book::new().moves(&pos).is_empty());
}

#[test]
fn position_with_only_zero_weight_or_zero_move_entries_lists_nothing() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let book = Book::from_entries(vec![entry(k, code(&pos, "e2e4"), 0), entry(k, 0, 9)]);
    assert!(book.moves(&pos).is_empty());
}

#[test]
fn percent_uses_the_total_including_illegal_and_zero_weight_entries() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let book = Book::from_entries(vec![entry(k, code(&pos, "e2e4"), 1), entry(k, 0x0abc, 1)]);
    assert_eq!(book.moves(&pos)[0].percent, 50);
    let single = book_with(&pos, &[("e2e4", 777)]);
    assert_eq!(single.moves(&pos)[0].percent, 100);
}

#[test]
fn percent_rounds_exact_halves_to_even_like_printf() {
    // total 8: 1/8 = 12.5 -> 12, 3/8 = 37.5 -> 38, 4/8 = 50.
    let pos = Position::new();
    let book = book_with(&pos, &[("e2e4", 1), ("d2d4", 3), ("c2c4", 4)]);
    let p: Vec<u8> = book.moves(&pos).iter().map(|m| m.percent).collect();
    assert_eq!(p, [12, 38, 50]);
    // total 200: 0.5 -> 0, 1.5 -> 2, 2.5 -> 2, 3.5 -> 4, 4.5 -> 4, 95.5 -> 96...
    let weights = [1u16, 3, 5, 7, 9, 175];
    let ucis = ["e2e4", "d2d4", "c2c4", "g1f3", "b1c3", "a2a3"];
    let moves: Vec<(&str, u16)> = ucis.iter().copied().zip(weights).collect();
    let book = book_with(&pos, &moves);
    assert_eq!(weights.iter().map(|w| u32::from(*w)).sum::<u32>(), 200);
    let p: Vec<u8> = book.moves(&pos).iter().map(|m| m.percent).collect();
    assert_eq!(p, [0, 2, 2, 4, 4, 88]);
    // Non-halves round normally.
    let book = book_with(&pos, &[("e2e4", 1), ("d2d4", 2)]);
    let p: Vec<u8> = book.moves(&pos).iter().map(|m| m.percent).collect();
    assert_eq!(p, [33, 67]);
}

#[test]
fn percent_does_not_overflow_with_maximum_weights() {
    let pos = Position::new();
    let book = book_with(
        &pos,
        &[("e2e4", 0xfff0), ("d2d4", 0xfff0), ("c2c4", 0xfff0)],
    );
    let p: Vec<u8> = book.moves(&pos).iter().map(|m| m.percent).collect();
    assert_eq!(p, [33, 33, 33]);
    // Many entries whose total exceeds u32.
    let k = polyglot_key(&pos);
    let many: Vec<Entry> = (0..70_000)
        .map(|_| entry(k, 0x0abc, 0xffff))
        .chain([entry(k, code(&pos, "e2e4"), 1)])
        .collect();
    let big = Book::from_entries(many);
    assert_eq!(big.moves(&pos)[0].percent, 0);
}

#[test]
fn binary_search_finds_the_first_of_many_entries_with_the_same_key() {
    let pos = Position::new();
    let target = polyglot_key(&pos);
    let ucis = ["e2e4", "d2d4", "c2c4", "g1f3", "b1c3"];
    let mut entries: Vec<Entry> = Vec::new();
    for i in 0..300u64 {
        entries.push(entry(i * 1_000_003 + 1, 1, 1));
    }
    for (i, u) in ucis.iter().enumerate() {
        entries.push(entry(target, code(&pos, u), 10 + i as u16));
    }
    for i in 0..300u64 {
        entries.push(entry(target.wrapping_add(i + 1) | 1, 1, 1));
    }
    let book = Book::from_entries(entries);
    let moves = book.moves(&pos);
    assert_eq!(moves.len(), 5, "all five entries of the key, none skipped");
    assert_eq!(moves[0].mv, mv(&pos, "e2e4"));
    assert_eq!(moves[4].mv, mv(&pos, "b1c3"));
    assert_eq!(book.entries_for(&pos).len(), 5);
}

#[test]
fn lookup_works_at_the_first_and_last_record_of_the_file() {
    let a = Position::new();
    let b = pos_after(&["e2e4"]);
    let a_is_low = polyglot_key(&a) < polyglot_key(&b);
    let (lo, hi) = if a_is_low { (&a, &b) } else { (&b, &a) };
    let first = if a_is_low { "e2e4" } else { "e7e5" };
    let last = if a_is_low { "e7e5" } else { "e2e4" };
    let key_lo = polyglot_key(lo);
    let key_hi = polyglot_key(hi);
    let book = Book::from_entries(vec![
        entry(key_lo, code(lo, first), 3),
        entry(key_hi, code(hi, last), 4),
    ]);
    assert_eq!(book.moves(lo).len(), 1);
    assert_eq!(book.moves(hi).len(), 1);
    assert_eq!(book.moves(lo)[0].weight, 3);
    assert_eq!(book.moves(hi)[0].weight, 4);
}

#[test]
fn corrupted_entries_among_valid_ones_do_not_break_listing() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let mut entries = vec![entry(k, code(&pos, "e2e4"), 5)];
    for junk in [0xffffu16, 0x8000, 0x7000, 0x0001, 0x0fff] {
        entries.push(entry(k, junk, 0xffff));
    }
    let book = Book::from_entries(entries);
    let moves = book.moves(&pos);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].mv, mv(&pos, "e2e4"));
}

#[test]
fn chess960_positions_have_no_book_moves_even_with_a_matching_key() {
    // Start 518 is the standard setup, so the key collides with the standard
    // one; the book still does not apply (D1104).
    let std = Position::new();
    let book = book_with(&std, &[("e2e4", 10)]);
    let p960 = Position::chess960_start(518).expect("start");
    assert_eq!(polyglot_key(&p960), polyglot_key(&std));
    assert!(book.moves(&p960).is_empty());
    assert!(book.pick(&p960, |_| 0).is_none());
    assert!(book.best(&p960).is_none());
    assert!(book.leading_to_book(&p960).is_empty());
    assert!(!book.moves(&std).is_empty());
}

// ----------------------------------------------------- leading_to_book

#[test]
fn leading_to_book_lists_legal_moves_into_positions_with_entries() {
    let pos = Position::new();
    let after_e4 = pos_after(&["e2e4"]);
    let after_d4 = pos_after(&["d2d4"]);
    let after_nf3 = pos_after(&["g1f3"]);
    let book = Book::from_entries(vec![
        // Any entry counts: weight 0, move 0 and illegal codes included.
        entry(polyglot_key(&after_e4), 0, 0),
        entry(polyglot_key(&after_d4), 0x0abc, 0),
        entry(polyglot_key(&after_nf3), code(&after_nf3, "g8f6"), 9),
        // A key unrelated to any child of the start.
        entry(0x1234, 1, 1),
    ]);
    let mut got: Vec<String> = book
        .leading_to_book(&pos)
        .into_iter()
        .map(|m| m.to_uci(pos.castling_mode()))
        .collect();
    got.sort();
    assert_eq!(got, ["d2d4", "e2e4", "g1f3"]);
}

#[test]
fn leading_to_book_does_not_depend_on_the_move_being_in_the_book() {
    let pos = Position::new();
    let after_e4 = pos_after(&["e2e4"]);
    // The start position itself has no entry for e4.
    let book = book_with(&after_e4, &[("e7e5", 1)]);
    assert!(book.moves(&pos).is_empty());
    assert_eq!(book.leading_to_book(&pos), [mv(&pos, "e2e4")]);
}

#[test]
fn leading_to_book_is_empty_for_empty_books_and_unrelated_entries() {
    let pos = Position::new();
    assert!(Book::new().leading_to_book(&pos).is_empty());
    assert!(
        Book::from_entries(vec![entry(42, 1, 1)])
            .leading_to_book(&pos)
            .is_empty()
    );
}

#[test]
fn leading_to_book_ignores_entries_of_the_position_itself() {
    let pos = Position::new();
    let book = book_with(&pos, &[("e2e4", 10)]);
    assert!(book.leading_to_book(&pos).is_empty());
}

#[test]
fn leading_to_book_includes_castling_and_promotion_children() {
    let pos = Position::from_fen("r3k2r/1P6/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
    let castle = mv(&pos, "e1g1");
    let promo = mv(&pos, "b7b8q");
    let book = Book::from_entries(vec![
        entry(polyglot_key(&pos.after(castle).expect("play")), 1, 1),
        entry(polyglot_key(&pos.after(promo).expect("play")), 1, 1),
    ]);
    let got = book.leading_to_book(&pos);
    assert_eq!(got.len(), 2);
    assert!(got.contains(&castle) && got.contains(&promo));
}

// ---------------------------------------------------------- pick / best

/// Runs `pick` for every roll in `0..bound`, returns (bound, picks by uci).
fn exhaustive_pick(book: &Book, pos: &Position) -> (u64, Vec<(String, u64)>) {
    let bound = Cell::new(0u64);
    book.pick(pos, |b| {
        bound.set(b);
        0
    })
    .expect("non-empty");
    let n = bound.get();
    let mut counts: Vec<(String, u64)> = Vec::new();
    for roll in 0..n {
        let m = book.pick(pos, |_| roll).expect("pick");
        let uci = m.mv.to_uci(pos.castling_mode());
        match counts.iter_mut().find(|(u, _)| *u == uci) {
            Some((_, c)) => *c += 1,
            None => counts.push((uci, 1)),
        }
    }
    (n, counts)
}

#[test]
fn pick_is_exactly_proportional_to_the_raw_weights() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let book = Book::from_entries(vec![
        entry(k, code(&pos, "e2e4"), 30),
        entry(k, 0x0abc, 7), // illegal: never picked
        entry(k, code(&pos, "d2d4"), 10),
        entry(k, code(&pos, "c2c4"), 0), // zero weight: never picked
    ]);
    let (bound, mut counts) = exhaustive_pick(&book, &pos);
    counts.sort();
    assert_eq!(bound, 40, "bound is the exact sum of the pickable weights");
    assert_eq!(counts, [("d2d4".to_string(), 10), ("e2e4".to_string(), 30)]);
}

#[test]
fn pick_boundaries_have_no_bias() {
    let pos = Position::new();
    let book = book_with(&pos, &[("e2e4", 30), ("d2d4", 10)]);
    let name = |roll| book.pick(&pos, |_| roll).map(|m| m.weight);
    assert_eq!(name(0), Some(30));
    assert_eq!(name(29), Some(30));
    assert_eq!(name(30), Some(10));
    assert_eq!(name(39), Some(10));
}

#[test]
fn pick_with_all_weights_equal_to_one_is_uniform() {
    let pos = Position::new();
    let ucis = ["e2e4", "d2d4", "c2c4", "g1f3", "b1c3", "a2a3"];
    let moves: Vec<(&str, u16)> = ucis.iter().map(|u| (*u, 1)).collect();
    let book = book_with(&pos, &moves);
    let (bound, counts) = exhaustive_pick(&book, &pos);
    assert_eq!(bound, 6);
    assert_eq!(counts.len(), 6);
    assert!(counts.iter().all(|(_, c)| *c == 1));
}

#[test]
fn pick_handles_maximum_weights_and_oversized_rolls() {
    let pos = Position::new();
    let book = book_with(
        &pos,
        &[("e2e4", 0xfff0), ("d2d4", 0xfff0), ("c2c4", 0xfff0)],
    );
    let bound = Cell::new(0);
    assert!(
        book.pick(&pos, |b| {
            bound.set(b);
            0
        })
        .is_some()
    );
    assert_eq!(bound.get(), 3 * 0xfff0);
    // A generator that ignores the bound must not panic nor yield None.
    assert!(book.pick(&pos, |_| u64::MAX).is_some());
    assert!(book.pick(&pos, |b| b).is_some());
}

#[test]
fn pick_without_candidates_is_none() {
    let pos = Position::new();
    assert!(Book::new().pick(&pos, |_| 0).is_none());
    let other = pos_after(&["e2e4"]);
    assert!(
        book_with(&other, &[("e7e5", 3)])
            .pick(&pos, |_| 0)
            .is_none()
    );
    let k = polyglot_key(&pos);
    let weightless = Book::from_entries(vec![
        entry(k, code(&pos, "e2e4"), 0),
        entry(k, 0, 5),
        entry(k, 0x0abc, 5),
    ]);
    assert!(weightless.pick(&pos, |_| 0).is_none());
    assert!(weightless.best(&pos).is_none());
    assert!(Book::new().best(&pos).is_none());
}

#[test]
fn best_takes_the_heaviest_and_ties_go_to_the_first_in_the_file() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let book = Book::from_entries(vec![
        entry(k, code(&pos, "d2d4"), 10),
        entry(k, 0x0abc, 0xffff), // illegal, heavier: ignored
        entry(k, code(&pos, "e2e4"), 30),
        entry(k, code(&pos, "c2c4"), 30),
    ]);
    assert_eq!(book.best(&pos).expect("best").mv, mv(&pos, "e2e4"));
    let single = book_with(&pos, &[("g1f3", 1)]);
    assert_eq!(single.best(&pos).expect("best").mv, mv(&pos, "g1f3"));
    // Equal weights everywhere: the first of the file.
    let eq = book_with(&pos, &[("b1c3", 1), ("e2e4", 1), ("d2d4", 1)]);
    assert_eq!(eq.best(&pos).expect("best").mv, mv(&pos, "b1c3"));
}

// ------------------------------------------------------------ validate

#[test]
fn validate_reports_order_tail_zero_moves_and_zero_weights() {
    let mut bytes = Vec::new();
    for (key, mvc, w) in [(5u64, 1u16, 1u16), (3, 0, 4), (3, 2, 0), (9, 0, 0)] {
        bytes.extend_from_slice(&key.to_be_bytes());
        bytes.extend_from_slice(&mvc.to_be_bytes());
        bytes.extend_from_slice(&w.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
    }
    bytes.extend_from_slice(&[1, 2, 3, 4, 5]);
    let report = Book::from_bytes(&bytes).expect("parse").validate();
    assert_eq!(report.entries, 4);
    assert_eq!(report.trailing_bytes, 5);
    assert!(!report.was_sorted);
    assert_eq!(report.zero_moves, 2);
    assert_eq!(report.zero_weights, 2);
    assert!(!report.is_clean());
}

#[test]
fn validate_of_a_clean_book_is_clean() {
    let pos = Position::new();
    let book = book_with(&pos, &[("e2e4", 3), ("d2d4", 2)]);
    let report = book.validate();
    assert!(report.is_clean());
    assert_eq!(report.entries, 2);
    assert_eq!(
        (
            report.trailing_bytes,
            report.zero_moves,
            report.zero_weights
        ),
        (0, 0, 0)
    );
    assert!(Book::new().validate().is_clean());
}

#[test]
fn illegal_moves_in_lists_the_stored_codes_that_do_not_apply() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let book = Book::from_entries(vec![
        entry(k, code(&pos, "e2e4"), 3),
        entry(k, 0x0abc, 1),
        entry(k, 0, 1),
        entry(k, 0x0123, 1),
    ]);
    // Code 0 is "absent", not an illegal move.
    assert_eq!(book.illegal_moves_in(&pos), [0x0abc, 0x0123]);
    assert!(book.illegal_moves_in(&pos_after(&["e2e4"])).is_empty());
}
