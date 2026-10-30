//! Editing: `set_position_moves`, `weights_from_percent`, `add_line` and
//! `remove_line` (D1108, D1109, D1116).

#![allow(clippy::cast_possible_truncation, clippy::assert_is_empty)]

mod common;

use common::{code, is_sorted_by_key, line, mv, pos_after, triples};
use mychess_book::{
    Book, BookError, Entry, MAX_MOVES_PER_POSITION, polyglot_key, weights_from_percent,
};
use mychess_core::{Move, Position};

/// A famous position with 218 legal moves (more than the 100-move limit).
const MANY_MOVES_FEN: &str = "R6R/3Q4/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp1Q4/kBNN1KB1 w - - 0 1";

fn learned(key: u64, mv: u16, weight: u16, learn: u32) -> Entry {
    Entry {
        key,
        mv,
        weight,
        learn,
    }
}

// ------------------------------------------------- set_position_moves

#[test]
fn set_position_moves_replaces_the_whole_set_in_the_given_order() {
    let pos = Position::new();
    let mut book = Book::new();
    book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 10), (mv(&pos, "d2d4"), 5)])
        .expect("set");
    // d4 changes weight, e4 is omitted (removed), c4 is new.
    book.set_position_moves(&pos, &[(mv(&pos, "d2d4"), 9), (mv(&pos, "c2c4"), 3)])
        .expect("replace");
    let moves = book.moves(&pos);
    assert_eq!(moves.len(), 2);
    assert_eq!((moves[0].mv, moves[0].weight), (mv(&pos, "d2d4"), 9));
    assert_eq!((moves[1].mv, moves[1].weight), (mv(&pos, "c2c4"), 3));
    assert_eq!(book.len(), 2);
}

#[test]
fn setting_an_empty_list_removes_every_move_of_the_position() {
    let pos = Position::new();
    let mut book = Book::new();
    book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 10), (mv(&pos, "d2d4"), 5)])
        .expect("set");
    book.set_position_moves(&pos, &[]).expect("clear");
    assert!(book.is_empty());
    assert!(book.moves(&pos).is_empty());
}

#[test]
fn other_positions_stay_intact_and_the_book_stays_sorted() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    // Neighbours below and above the target key, with learn data.
    let below = learned(k - 1, 0x0101, 11, 0xaaaa_bbbb);
    let above = learned(k + 1, 0x0202, 22, 0xcccc_dddd);
    let far_high = learned(u64::MAX, 0x0303, 33, 0x0102_0304);
    let far_low = learned(0, 0x0404, 44, 0x0506_0708);
    let mut book = Book::from_entries(vec![far_low, below, above, far_high]);
    book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 10)])
        .expect("insert in the middle");
    assert!(is_sorted_by_key(&book));
    assert!(book.was_sorted());
    let others: Vec<Entry> = book
        .entries()
        .iter()
        .copied()
        .filter(|e| e.key != k)
        .collect();
    assert_eq!(others, [far_low, below, above, far_high]);
    // And removing again leaves exactly the originals.
    book.set_position_moves(&pos, &[]).expect("remove");
    assert_eq!(book.entries(), [far_low, below, above, far_high]);
}

#[test]
fn rewritten_entries_lose_their_learn_data_but_others_keep_it() {
    let pos = Position::new();
    let k = polyglot_key(&pos);
    let e4 = code(&pos, "e2e4");
    let d4 = code(&pos, "d2d4");
    let kept = learned(k + 5, 9, 9, 0x1111_2222);
    let mut book = Book::from_entries(vec![
        learned(k, e4, 10, 0xffff_ffff),
        learned(k, d4, 10, 0x0001_0001),
        kept,
    ]);
    book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 20)])
        .expect("set");
    let rewritten: Vec<&Entry> = book.entries().iter().filter(|e| e.key == k).collect();
    assert_eq!(rewritten.len(), 1);
    assert_eq!(
        (rewritten[0].mv, rewritten[0].weight, rewritten[0].learn),
        (e4, 20, 0)
    );
    assert!(book.entries().contains(&kept));
}

#[test]
fn removing_moves_shortens_the_serialised_book() {
    let pos = Position::new();
    let mut book = Book::new();
    book.set_position_moves(
        &pos,
        &[
            (mv(&pos, "e2e4"), 1),
            (mv(&pos, "d2d4"), 1),
            (mv(&pos, "c2c4"), 1),
        ],
    )
    .expect("set");
    assert_eq!(book.to_bytes().len(), 48);
    book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 1)])
        .expect("shrink");
    assert_eq!(book.to_bytes().len(), 16);
}

#[test]
fn zero_weight_is_refused_and_the_book_is_unchanged() {
    let pos = Position::new();
    let mut book = Book::new();
    book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 5)])
        .expect("set");
    let before = book.to_bytes();
    let err = book
        .set_position_moves(&pos, &[(mv(&pos, "d2d4"), 3), (mv(&pos, "e2e4"), 0)])
        .expect_err("zero weight");
    assert!(matches!(err, BookError::ZeroWeight), "{err:?}");
    assert_eq!(
        book.to_bytes(),
        before,
        "a failed edit leaves the book as it was"
    );
}

#[test]
fn weight_limits_one_and_0xfff0_are_accepted_above_is_not() {
    let pos = Position::new();
    let mut book = Book::new();
    book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 1), (mv(&pos, "d2d4"), 0xfff0)])
        .expect("limits");
    let err = book
        .set_position_moves(&pos, &[(mv(&pos, "e2e4"), 0xfff1)])
        .expect_err("too heavy");
    assert!(
        matches!(err, BookError::WeightTooLarge { weight: 0xfff1 }),
        "{err:?}"
    );
    assert_eq!(book.len(), 2);
}

#[test]
fn duplicate_and_illegal_moves_are_rejected_without_changes() {
    let pos = Position::new();
    let e4 = mv(&pos, "e2e4");
    let mut book = Book::new();
    book.set_position_moves(&pos, &[(e4, 5)]).expect("set");
    let before = book.to_bytes();
    assert!(matches!(
        book.set_position_moves(&pos, &[(e4, 1), (e4, 2)]),
        Err(BookError::DuplicateMove)
    ));
    // A legal move of ANOTHER position is illegal here.
    let other = pos_after(&["e2e4"]);
    let foreign = mv(&other, "e7e5");
    assert!(book.set_position_moves(&pos, &[(foreign, 1)]).is_err());
    assert!(book.set_position_moves(&pos, &[(Move::Null, 1)]).is_err());
    assert_eq!(book.to_bytes(), before);
}

#[test]
fn up_to_one_hundred_moves_are_accepted_and_one_hundred_one_is_not() {
    assert_eq!(MAX_MOVES_PER_POSITION, 100);
    let pos = Position::from_fen(MANY_MOVES_FEN).expect("fen");
    let legal = pos.legal_moves();
    assert!(legal.len() > 101, "fixture must have more than 101 moves");
    let with = |n: usize| -> Vec<(Move, u16)> { legal.iter().take(n).map(|m| (*m, 1)).collect() };
    let mut book = Book::new();
    book.set_position_moves(&pos, &with(100))
        .expect("exactly 100");
    assert_eq!(book.len(), 100);
    let err = book.set_position_moves(&pos, &with(101)).expect_err("101");
    assert!(
        matches!(err, BookError::TooManyMoves { count: 101 }),
        "{err:?}"
    );
    assert_eq!(book.len(), 100, "unchanged after the error");
    let err = book
        .set_position_moves(&pos, &with(legal.len()))
        .expect_err("all");
    assert!(matches!(err, BookError::TooManyMoves { .. }));
}

#[test]
fn chess960_positions_are_unsupported_for_editing() {
    let p = Position::chess960_start(518).expect("start");
    let m = p.legal_moves()[0];
    let mut book = Book::new();
    assert!(matches!(
        book.set_position_moves(&p, &[(m, 1)]),
        Err(BookError::UnsupportedPosition)
    ));
    assert!(matches!(
        book.add_line(&[m], &p, 1),
        Err(BookError::UnsupportedPosition)
    ));
    assert!(book.is_empty());
}

#[test]
fn castling_and_promotion_moves_can_be_stored_and_listed() {
    let pos = Position::from_fen("r3k2r/1P6/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
    let castle = mv(&pos, "e1g1");
    let promo = mv(&pos, "b7b8n");
    let mut book = Book::new();
    book.set_position_moves(&pos, &[(castle, 3), (promo, 4)])
        .expect("set");
    assert_eq!(book.entries()[0].mv, 0x0107);
    let listed: Vec<Move> = book.moves(&pos).iter().map(|m| m.mv).collect();
    assert_eq!(listed, [castle, promo]);
}

// ------------------------------------------------------ read-only

#[test]
fn read_only_books_refuse_every_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("ro.bin");
    let pos = Position::new();
    let e4 = mv(&pos, "e2e4");
    let mut book = Book::new();
    book.set_position_moves(&pos, &[(e4, 5)]).expect("set");
    book.save_as(&path).expect("save");
    let mut ro = Book::open_readonly(&path).expect("open");
    let before = ro.to_bytes();
    assert!(matches!(
        ro.set_position_moves(&pos, &[(e4, 9)]),
        Err(BookError::ReadOnly)
    ));
    assert!(matches!(
        ro.add_line(&[e4], &pos, 1),
        Err(BookError::ReadOnly)
    ));
    assert!(matches!(
        ro.remove_line(&[e4], &pos),
        Err(BookError::ReadOnly)
    ));
    assert_eq!(ro.to_bytes(), before);
}

// ----------------------------------------------- weights_from_percent

#[test]
fn weights_from_percent_uses_the_0xfff0_scale() {
    let pos = Position::new();
    let (a, b, c) = (mv(&pos, "e2e4"), mv(&pos, "d2d4"), mv(&pos, "c2c4"));
    let w = weights_from_percent(&[(a, 50), (b, 25), (c, 0)]);
    assert_eq!(w, [(a, 0xfff0), (b, 0x7ff8), (c, 1)]);
    // Scale relative to the largest probability, not to 100.
    let w = weights_from_percent(&[(a, 100), (b, 33)]);
    assert_eq!(w, [(a, 0xfff0), (b, (33u32 * 0xfff0 / 100) as u16)]);
    assert_eq!(w[1].1, 21_621, "integer division, no rounding up");
    let w = weights_from_percent(&[(a, 1), (b, 100)]);
    assert_eq!(w[0].1, 655);
    let w = weights_from_percent(&[(a, 255)]);
    assert_eq!(w, [(a, 0xfff0)]);
}

#[test]
fn weights_from_percent_with_all_zero_gives_weight_one() {
    let pos = Position::new();
    let (a, b) = (mv(&pos, "e2e4"), mv(&pos, "d2d4"));
    assert_eq!(weights_from_percent(&[(a, 0)]), [(a, 1)]);
    assert_eq!(weights_from_percent(&[(a, 0), (b, 0)]), [(a, 1), (b, 1)]);
    assert!(weights_from_percent(&[]).is_empty());
}

#[test]
fn weights_from_percent_never_yields_zero_or_more_than_the_maximum() {
    let pos = Position::new();
    let moves: Vec<(Move, u8)> = pos
        .legal_moves()
        .into_iter()
        .enumerate()
        .map(|(i, m)| (m, (i * 13 % 256) as u8))
        .collect();
    for (_, w) in weights_from_percent(&moves) {
        assert!((1..=0xfff0).contains(&w), "{w}");
    }
}

#[test]
fn percent_weights_can_be_stored_directly() {
    let pos = Position::new();
    let (a, b) = (mv(&pos, "e2e4"), mv(&pos, "d2d4"));
    let mut book = Book::new();
    book.set_position_moves(&pos, &weights_from_percent(&[(a, 60), (b, 0)]))
        .expect("storable");
    assert_eq!(book.moves(&pos).len(), 2);
}

// ----------------------------------------------- add_line / remove_line

#[test]
fn add_line_inserts_one_entry_per_ply_with_the_given_weight() {
    let start = Position::new();
    let l = line(&["e2e4", "e7e5", "g1f3"]);
    let mut book = Book::new();
    assert_eq!(book.add_line(&l, &start, 7).expect("add"), 3);
    assert_eq!(book.len(), 3);
    assert!(is_sorted_by_key(&book));
    assert!(book.entries().iter().all(|e| e.weight == 7 && e.learn == 0));
    let mut pos = start;
    for m in &l {
        let listed = book.moves(&pos);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].mv, *m);
        pos.play(*m).expect("play");
    }
}

#[test]
fn add_line_only_inserts_missing_moves_and_keeps_existing_weights() {
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    let after = start.after(e4).expect("play");
    let c5 = mv(&after, "c7c5");
    let mut book = Book::new();
    book.set_position_moves(&start, &[(e4, 500), (mv(&start, "d2d4"), 40)])
        .expect("seed");
    let added = book.add_line(&[e4, c5], &start, 1).expect("add");
    assert_eq!(added, 1, "only c5 is new");
    let moves = book.moves(&start);
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0].weight, 500, "existing weight is kept");
    assert_eq!(moves[1].weight, 40, "siblings are untouched");
    assert_eq!(book.moves(&after)[0].weight, 1);
    // Adding again changes nothing.
    let before = book.to_bytes();
    assert_eq!(book.add_line(&[e4, c5], &start, 9).expect("again"), 0);
    assert_eq!(book.to_bytes(), before);
}

#[test]
fn a_new_move_goes_after_the_existing_ones_of_its_position() {
    let start = Position::new();
    let mut book = Book::new();
    book.set_position_moves(&start, &[(mv(&start, "e2e4"), 5)])
        .expect("seed");
    book.add_line(&[mv(&start, "d2d4")], &start, 2)
        .expect("add");
    let order: Vec<Move> = book.moves(&start).iter().map(|m| m.mv).collect();
    assert_eq!(order, [mv(&start, "e2e4"), mv(&start, "d2d4")]);
}

#[test]
fn add_line_from_a_non_initial_position() {
    let start = pos_after(&["e2e4", "e7e5"]);
    let l = vec![mv(&start, "g1f3")];
    let mut book = Book::new();
    assert_eq!(book.add_line(&l, &start, 3).expect("add"), 1);
    assert_eq!(book.moves(&start)[0].mv, l[0]);
    assert!(book.moves(&Position::new()).is_empty());
}

#[test]
fn a_line_that_revisits_a_position_does_not_insert_duplicates() {
    let start = Position::new();
    let l = line(&["g1f3", "g8f6", "f3g1", "f6g8", "g1f3"]);
    let mut book = Book::new();
    assert_eq!(book.add_line(&l, &start, 1).expect("add"), 4);
    assert_eq!(book.len(), 4);
    assert_eq!(book.remove_line(&l, &start).expect("remove"), 4);
    assert!(book.is_empty());
}

#[test]
fn add_line_with_an_illegal_move_changes_nothing() {
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    let mut book = Book::new();
    book.set_position_moves(&start, &[(mv(&start, "d2d4"), 8)])
        .expect("seed");
    let before = book.to_bytes();
    // e4 is legal at ply 0, but e4 again at ply 1 (black to move) is not.
    let err = book.add_line(&[e4, e4], &start, 1).expect_err("illegal");
    assert!(matches!(err, BookError::IllegalMove { ply: 1 }), "{err:?}");
    assert_eq!(book.to_bytes(), before, "all or nothing");
    let err = book
        .add_line(&[e4, Move::Null], &start, 1)
        .expect_err("null move");
    assert!(matches!(err, BookError::IllegalMove { ply: 1 }), "{err:?}");
    assert_eq!(book.to_bytes(), before);
}

#[test]
fn add_line_rejects_invalid_weights_without_changes() {
    let start = Position::new();
    let l = line(&["e2e4"]);
    let mut book = Book::new();
    assert!(matches!(
        book.add_line(&l, &start, 0),
        Err(BookError::ZeroWeight)
    ));
    assert!(matches!(
        book.add_line(&l, &start, 0xfff1),
        Err(BookError::WeightTooLarge { .. })
    ));
    assert!(book.is_empty());
}

#[test]
fn an_empty_line_is_a_no_op() {
    let start = Position::new();
    let mut book = Book::new();
    assert_eq!(book.add_line(&[], &start, 1).expect("add"), 0);
    assert_eq!(book.remove_line(&[], &start).expect("remove"), 0);
    assert!(book.is_empty());
}

#[test]
fn remove_line_removes_only_the_line_moves_and_keeps_siblings() {
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    let d4 = mv(&start, "d2d4");
    let after = start.after(e4).expect("play");
    let e5 = mv(&after, "e7e5");
    let c5 = mv(&after, "c7c5");
    let mut book = Book::new();
    book.set_position_moves(&start, &[(e4, 10), (d4, 20)])
        .expect("seed");
    book.set_position_moves(&after, &[(e5, 3), (c5, 4)])
        .expect("seed");
    assert_eq!(book.remove_line(&[e4, e5], &start).expect("remove"), 2);
    let root: Vec<Move> = book.moves(&start).iter().map(|m| m.mv).collect();
    let reply: Vec<Move> = book.moves(&after).iter().map(|m| m.mv).collect();
    assert_eq!(root, [d4]);
    assert_eq!(reply, [c5]);
    assert!(is_sorted_by_key(&book));
    // Nothing left to remove.
    assert_eq!(book.remove_line(&[e4, e5], &start).expect("again"), 0);
}

#[test]
fn remove_line_is_all_or_nothing_on_illegal_lines() {
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    let mut book = Book::new();
    book.set_position_moves(&start, &[(e4, 10)]).expect("seed");
    let before = book.to_bytes();
    let err = book.remove_line(&[e4, e4], &start).expect_err("illegal");
    assert!(matches!(err, BookError::IllegalMove { ply: 1 }), "{err:?}");
    assert_eq!(book.to_bytes(), before);
}

#[test]
fn line_edits_touch_memory_only_until_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("lines.bin");
    let start = Position::new();
    let mut book = Book::new();
    book.set_position_moves(&start, &[(mv(&start, "d2d4"), 5)])
        .expect("seed");
    book.save_as(&path).expect("save");
    let on_disk = std::fs::read(&path).expect("read");

    let l = line(&["e2e4", "e7e5", "g1f3", "b8c6"]);
    assert_eq!(book.add_line(&l, &start, 2).expect("add"), 4);
    assert_eq!(std::fs::read(&path).expect("read"), on_disk, "no write yet");
    book.save().expect("save");
    let reopened = Book::open(&path).expect("open");
    assert_eq!(reopened.len(), 5);
    assert_eq!(triples(&reopened), triples(&book));
    assert!(reopened.was_sorted());
    assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1);
}

#[test]
fn entries_of_untouched_positions_survive_line_edits_byte_for_byte() {
    let start = Position::new();
    let probe = learned(0x1234_5678, 0x0101, 77, 0xfeed_f00d);
    let mut book = Book::from_entries(vec![probe]);
    book.add_line(&line(&["e2e4", "e7e5"]), &start, 1)
        .expect("add");
    assert!(book.entries().contains(&probe));
    book.remove_line(&line(&["e2e4", "e7e5"]), &start)
        .expect("remove");
    assert_eq!(book.entries(), [probe]);
}
