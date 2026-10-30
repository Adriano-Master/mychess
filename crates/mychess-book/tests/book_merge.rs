//! Merging two books (D1111, D1117).

#![allow(clippy::assert_is_empty)]

mod common;

use common::{entry, is_sorted_by_key, triples};
use mychess_book::{Book, MergeMode, merge};

fn a_and_b() -> (Book, Book) {
    (
        Book::from_entries(vec![
            entry(1, 1, 10),
            entry(2, 2, 6), // file order inside the key: weight 6 before 5
            entry(2, 1, 5),
            entry(5, 7, 1),
        ]),
        Book::from_entries(vec![
            entry(2, 1, 4),
            entry(2, 3, 9),
            entry(3, 1, 1),
            entry(3, 2, 2),
            entry(5, 7, 8),
            entry(9, 4, 3),
        ]),
    )
}

#[test]
fn keep_first_uses_book_one_entirely_for_common_keys() {
    let (a, b) = a_and_b();
    let (m, r) = merge(&a, &b, MergeMode::KeepFirst);
    assert_eq!(
        triples(&m),
        [
            (1, 1, 10),
            (2, 2, 6),
            (2, 1, 5),
            (3, 1, 1),
            (3, 2, 2),
            (5, 7, 1),
            (9, 4, 3),
        ]
    );
    assert_eq!(r.first_entries, 4);
    assert_eq!(r.second_entries, 6);
    assert_eq!(r.written, 7);
    assert_eq!(r.common_keys, 2);
    // Dropped from book two: (2,1), (2,3) and (5,7).
    assert_eq!(r.skipped, 3);
    assert!(m.was_sorted());
    assert!(is_sorted_by_key(&m));
}

#[test]
fn keep_first_is_the_default_mode() {
    assert_eq!(MergeMode::default(), MergeMode::KeepFirst);
}

#[test]
fn keep_first_with_disjoint_books_is_the_sorted_union() {
    let a = Book::from_entries(vec![entry(10, 1, 1), entry(30, 1, 1)]);
    let b = Book::from_entries(vec![entry(20, 1, 1), entry(40, 1, 2), entry(40, 2, 1)]);
    for mode in [MergeMode::KeepFirst, MergeMode::SumWeights] {
        let (m, r) = merge(&a, &b, mode);
        let keys: Vec<u64> = m.entries().iter().map(|e| e.key).collect();
        assert_eq!(keys, [10, 20, 30, 40, 40], "{mode:?}");
        // Entries of one key keep the order of their source book.
        assert_eq!(m.entries()[3].weight, 2, "{mode:?}");
        assert_eq!((r.skipped, r.common_keys, r.written), (0, 0, 5), "{mode:?}");
    }
}

#[test]
fn keep_first_book_two_wins_nowhere_on_common_keys_even_with_heavier_moves() {
    let a = Book::from_entries(vec![entry(1, 1, 1)]);
    let b = Book::from_entries(vec![entry(1, 1, 0xfff0), entry(1, 2, 0xfff0)]);
    let (m, r) = merge(&a, &b, MergeMode::KeepFirst);
    assert_eq!(triples(&m), [(1, 1, 1)]);
    assert_eq!(r.skipped, 2);
}

#[test]
fn sum_weights_adds_equal_moves_and_unites_distinct_ones() {
    let (a, b) = a_and_b();
    let (m, r) = merge(&a, &b, MergeMode::SumWeights);
    // Key 2: moves {2: 6, 1: 5+4=9, 3: 9} ordered by descending weight.
    let key2: Vec<(u16, u16)> = m
        .entries()
        .iter()
        .filter(|e| e.key == 2)
        .map(|e| (e.mv, e.weight))
        .collect();
    assert_eq!(key2.len(), 3);
    assert!(key2.windows(2).all(|w| w[0].1 >= w[1].1), "{key2:?}");
    assert!(key2.contains(&(1, 9)) && key2.contains(&(3, 9)) && key2.contains(&(2, 6)));
    // Key 5 is common too: same move 7, weights 1 + 8.
    let key5: Vec<_> = m.entries().iter().filter(|e| e.key == 5).collect();
    assert_eq!((key5.len(), key5[0].weight), (1, 9));
    // Keys present in one book only are copied unchanged.
    for (k, mv, w) in [(1, 1, 10), (9, 4, 3)] {
        assert!(triples(&m).contains(&(k, mv, w)));
    }
    assert_eq!(r.common_keys, 2);
    assert_eq!(r.skipped, 0);
    assert_eq!(r.written, m.len());
    assert_eq!(m.len(), 1 + 3 + 2 + 1 + 1);
    assert!(is_sorted_by_key(&m));
    assert!(m.was_sorted());
}

#[test]
fn sum_weights_saturates_at_0xfff0() {
    let a = Book::from_entries(vec![
        entry(1, 1, 0xfff0),
        entry(2, 1, 0xffef),
        entry(3, 1, 40_000),
    ]);
    let b = Book::from_entries(vec![
        entry(1, 1, 0xfff0),
        entry(2, 1, 3),
        entry(3, 1, 40_000),
    ]);
    let (m, _) = merge(&a, &b, MergeMode::SumWeights);
    assert_eq!(
        triples(&m),
        [(1, 1, 0xfff0), (2, 1, 0xfff0), (3, 1, 0xfff0)]
    );
}

#[test]
fn sum_weights_sorts_by_weight_only_the_common_keys() {
    // Key 1 is common; key 2 only in `a` with ascending weights; key 3 only in `b`.
    let a = Book::from_entries(vec![
        entry(1, 1, 1),
        entry(1, 2, 5),
        entry(2, 1, 1),
        entry(2, 2, 9),
    ]);
    let b = Book::from_entries(vec![entry(1, 3, 3), entry(3, 1, 2), entry(3, 2, 8)]);
    let (m, r) = merge(&a, &b, MergeMode::SumWeights);
    assert_eq!(
        triples(&m),
        [
            (1, 2, 5), // descending: 5, 3, 1
            (1, 3, 3),
            (1, 1, 1),
            (2, 1, 1), // untouched keys keep the source order
            (2, 2, 9),
            (3, 1, 2),
            (3, 2, 8),
        ]
    );
    assert_eq!(r.common_keys, 1);
}

#[test]
fn sum_weights_ties_keep_book_one_moves_first() {
    let a = Book::from_entries(vec![entry(1, 10, 4), entry(1, 11, 4)]);
    let b = Book::from_entries(vec![entry(1, 12, 4), entry(1, 10, 0)]);
    let (m, _) = merge(&a, &b, MergeMode::SumWeights);
    let moves: Vec<u16> = m.entries().iter().map(|e| e.mv).collect();
    assert_eq!(
        moves,
        [10, 11, 12],
        "stable sort: a's moves, then b's new ones"
    );
}

#[test]
fn merging_with_an_empty_book_returns_the_other_book() {
    let (a, _) = a_and_b();
    let empty = Book::new();
    for mode in [MergeMode::KeepFirst, MergeMode::SumWeights] {
        let (left, r) = merge(&a, &empty, mode);
        assert_eq!(left.entries(), a.entries(), "{mode:?}");
        assert_eq!((r.first_entries, r.second_entries, r.skipped), (4, 0, 0));
        let (right, r) = merge(&empty, &a, mode);
        assert_eq!(right.entries(), a.entries(), "{mode:?}");
        assert_eq!((r.first_entries, r.second_entries, r.skipped), (0, 4, 0));
        let (both, r) = merge(&empty, &empty, mode);
        assert!(both.is_empty());
        assert_eq!(r.written, 0);
    }
}

#[test]
fn keep_first_with_itself_is_idempotent() {
    let (a, _) = a_and_b();
    let (m, r) = merge(&a, &a, MergeMode::KeepFirst);
    assert_eq!(m.entries(), a.entries());
    assert_eq!(r.skipped, a.len());
    assert_eq!(r.common_keys, 3);
}

#[test]
fn sum_weights_with_itself_doubles_weights_but_not_the_move_set() {
    let (a, _) = a_and_b();
    let (m, r) = merge(&a, &a, MergeMode::SumWeights);
    assert_eq!(m.len(), a.len());
    for e in a.entries() {
        let doubled = m
            .entries()
            .iter()
            .find(|x| x.key == e.key && x.mv == e.mv)
            .expect("move kept");
        assert_eq!(doubled.weight, e.weight * 2);
    }
    assert_eq!(r.skipped, 0);
    assert_eq!(r.common_keys, 3);
}

#[test]
fn merge_does_not_modify_its_inputs_and_result_is_detached() {
    let (a, b) = a_and_b();
    let (ea, eb) = (a.entries().to_vec(), b.entries().to_vec());
    let (m, _) = merge(&a, &b, MergeMode::SumWeights);
    assert_eq!(a.entries(), ea);
    assert_eq!(b.entries(), eb);
    assert!(m.path().is_none());
    assert!(!m.is_read_only());
}

#[test]
fn merged_book_can_be_saved_and_reopened() {
    let (a, b) = a_and_b();
    let (mut m, _) = merge(&a, &b, MergeMode::KeepFirst);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("merged.bin");
    m.save_as(&path).expect("save");
    assert_eq!(Book::open(&path).expect("open").entries(), m.entries());
}

#[test]
fn merge_of_unsorted_inputs_uses_their_sorted_form() {
    let a = Book::from_entries(vec![entry(9, 1, 1), entry(1, 1, 1)]);
    let b = Book::from_entries(vec![entry(5, 1, 1), entry(1, 2, 1)]);
    assert!(!a.was_sorted());
    let (m, r) = merge(&a, &b, MergeMode::KeepFirst);
    assert_eq!(triples(&m), [(1, 1, 1), (5, 1, 1), (9, 1, 1)]);
    assert_eq!(r.skipped, 1);
}
