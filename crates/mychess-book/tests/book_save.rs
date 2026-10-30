//! Atomic writing and round-trips through files (D1110, D1116 e/f).

#![allow(clippy::cast_possible_truncation, clippy::assert_is_empty)]

mod common;

use std::path::Path;

use common::{book_with, mv};
use mychess_book::{Book, BookError, Entry, SaveFault};
use mychess_core::Position;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .expect("read_dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn sample() -> (Position, Book) {
    let pos = Position::new();
    let book = book_with(&pos, &[("e2e4", 30), ("d2d4", 10), ("c2c4", 5)]);
    (pos, book)
}

#[test]
fn save_as_writes_the_serialised_book_and_binds_the_path() {
    let (pos, mut book) = sample();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.bin");
    book.save_as(&path).expect("save_as");
    assert_eq!(std::fs::read(&path).expect("read"), book.to_bytes());
    assert_eq!(book.path(), Some(path.as_path()));
    assert_eq!(names(dir.path()), ["a.bin"], "no temporary left behind");
    assert_eq!(
        Book::open(&path).expect("open").moves(&pos),
        book.moves(&pos)
    );
}

#[test]
fn save_replaces_the_file_in_place_without_leftovers() {
    let (pos, mut book) = sample();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.bin");
    book.save_as(&path).expect("save_as");
    book.set_position_moves(&pos, &[(mv(&pos, "g1f3"), 9)])
        .expect("edit");
    book.save().expect("save");
    assert_eq!(names(dir.path()), ["a.bin"]);
    let reopened = Book::open(&path).expect("open");
    assert_eq!(reopened.len(), 1);
    assert_eq!(reopened.entries(), book.entries());
    assert_eq!(std::fs::metadata(&path).expect("meta").len(), 16);
}

#[test]
fn save_as_to_another_path_leaves_the_original_file_alone() {
    let (_, mut book) = sample();
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.bin");
    let second = dir.path().join("second.bin");
    book.save_as(&first).expect("save");
    let original = std::fs::read(&first).expect("read");
    book.set_position_moves(&Position::new(), &[])
        .expect("clear");
    book.save_as(&second).expect("save_as");
    assert_eq!(std::fs::read(&first).expect("read"), original);
    assert_eq!(std::fs::read(&second).expect("read").len(), 0);
    assert_eq!(book.path(), Some(second.as_path()));
    assert_eq!(names(dir.path()), ["first.bin", "second.bin"]);
}

#[test]
fn save_as_overwrites_an_existing_file() {
    let (_, mut book) = sample();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("x.bin");
    std::fs::write(&path, b"old contents that are not a book").expect("seed");
    book.save_as(&path).expect("save_as");
    assert_eq!(std::fs::read(&path).expect("read"), book.to_bytes());
    assert_eq!(names(dir.path()), ["x.bin"]);
}

#[test]
fn save_as_makes_a_read_only_book_writable_on_the_new_path() {
    let (pos, mut book) = sample();
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src.bin");
    book.save_as(&src).expect("save");
    let mut ro = Book::open_readonly(&src).expect("open");
    assert!(matches!(ro.save(), Err(BookError::ReadOnly)));
    let copy = dir.path().join("copy.bin");
    ro.save_as(&copy)
        .expect("save_as is allowed on a read-only book");
    assert!(!ro.is_read_only());
    ro.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 1)])
        .expect("edit");
    ro.save().expect("save to the new path");
    assert_eq!(Book::open(&copy).expect("open").len(), 1);
    assert_eq!(Book::open(&src).expect("open").len(), 3, "source untouched");
}

#[test]
fn save_without_a_path_is_no_path() {
    let (_, mut book) = sample();
    assert!(matches!(book.save(), Err(BookError::NoPath)));
    assert!(matches!(
        book.save_with_fault(SaveFault::BeforeRename),
        Err(BookError::NoPath)
    ));
}

#[test]
fn save_into_a_missing_directory_fails_cleanly() {
    let (_, mut book) = sample();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no_such_dir").join("a.bin");
    let err = book.save_as(&path).expect_err("missing dir");
    assert!(matches!(err, BookError::Io { .. }), "{err:?}");
    assert_eq!(names(dir.path()), Vec::<String>::new());
    assert!(
        book.path().is_none(),
        "a failed save_as does not bind the path"
    );
}

#[test]
fn injected_faults_keep_the_original_intact_and_leave_no_temporary() {
    for fault in [SaveFault::AfterPartialWrite, SaveFault::BeforeRename] {
        let (pos, mut book) = sample();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.bin");
        book.save_as(&path).expect("save_as");
        let original = std::fs::read(&path).expect("read");

        book.set_position_moves(&pos, &[(mv(&pos, "g1f3"), 1)])
            .expect("edit");
        let err = book.save_with_fault(fault).expect_err("fault");
        assert!(matches!(err, BookError::Io { .. }), "{fault:?}: {err:?}");
        assert_eq!(std::fs::read(&path).expect("read"), original, "{fault:?}");
        assert_eq!(names(dir.path()), ["a.bin"], "{fault:?}: no temporary");
        // The in-memory edit survives and a later real save works.
        assert_eq!(book.len(), 1);
        book.save().expect("retry");
        assert_eq!(Book::open(&path).expect("open").len(), 1);
        assert_eq!(names(dir.path()), ["a.bin"]);
    }
}

#[test]
fn read_only_book_refuses_save_even_with_a_fault_hook() {
    let (_, mut book) = sample();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.bin");
    book.save_as(&path).expect("save");
    let mut ro = Book::open_readonly(&path).expect("open");
    assert!(matches!(
        ro.save_with_fault(SaveFault::AfterPartialWrite),
        Err(BookError::ReadOnly)
    ));
    assert_eq!(names(dir.path()), ["a.bin"]);
}

#[test]
fn saving_drops_the_trailing_partial_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("tail.bin");
    let (_, book) = sample();
    let mut bytes = book.to_bytes();
    bytes.extend_from_slice(&[9; 11]);
    std::fs::write(&path, &bytes).expect("write");
    let mut opened = Book::open(&path).expect("open");
    assert_eq!(opened.validate().trailing_bytes, 11);
    opened.save().expect("save");
    assert_eq!(std::fs::read(&path).expect("read"), book.to_bytes());
    assert_eq!(opened.validate().trailing_bytes, 0);
    assert!(opened.validate().is_clean());
}

#[test]
fn open_save_round_trip_is_byte_identical_for_a_sorted_book() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rt.bin");
    // Sorted file with distinctive learn data and weights.
    let entries: Vec<Entry> = (0..40u64)
        .map(|i| Entry {
            key: i / 3 * 0x0101_0101_0101_0101,
            mv: (i as u16) * 31 + 1,
            weight: (i as u16) * 1000 % 0xfff0,
            learn: 0x0102_0304u32.wrapping_mul(i as u32 + 1),
        })
        .collect();
    let bytes = Book::from_entries(entries).to_bytes();
    std::fs::write(&path, &bytes).expect("write");
    let mut book = Book::open(&path).expect("open");
    assert!(book.was_sorted());
    book.save().expect("save");
    assert_eq!(std::fs::read(&path).expect("read"), bytes);
}

#[test]
fn the_file_is_big_endian_sixteen_byte_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("be.bin");
    let mut book = Book::from_entries(vec![Entry {
        key: 0x0102_0304_0506_0708,
        mv: 0x090a,
        weight: 0x0b0c,
        learn: 0x0d0e_0f10,
    }]);
    book.save_as(&path).expect("save");
    let expect: Vec<u8> = (1..=16).collect();
    assert_eq!(std::fs::read(&path).expect("read"), expect);
}

#[test]
fn saving_an_empty_book_writes_an_empty_file_that_cannot_be_reopened() {
    // Empty files are an error on open (D1101); saving one is still allowed.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("empty.bin");
    Book::new().save_as(&path).expect("save");
    assert_eq!(std::fs::metadata(&path).expect("meta").len(), 0);
    assert!(Book::open(&path).is_err());
}
