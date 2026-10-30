//! M7-5a: text editing, text search and atomic (gzip-aware) writing of EPD
//! books, through the public API only.

use std::fs;
use std::path::Path;

use mychess_core::{Position, SanMode};
use mychess_pgn::epd::{EpdBook, EpdDiagnosticKind, EpdOperation, EpdOptions, TextWarning};

fn after(sans: &str) -> Position {
    let mut p = Position::new();
    for s in sans.split_whitespace() {
        let m = p.parse_san(s, SanMode::Strict).expect("san").mv;
        p = p.after(m).expect("legal");
    }
    p
}

/// Five records like the spec's `cinco.epd`.
fn cinco() -> EpdBook {
    let mut b = EpdBook::new();
    b.insert(Position::new(), vec![EpdOperation::new("bm", "e4")]);
    b.insert(after("e4"), vec![EpdOperation::new("am", "f5")]);
    b.insert(
        after("d4"),
        vec![
            EpdOperation::new("id", "\"X;Y\""),
            EpdOperation::new("c0", "a\\b"),
        ],
    );
    b.insert(
        after("c4"),
        vec![
            EpdOperation::new("ce", "25"),
            EpdOperation::new("pv", "e5 Nf3"),
        ],
    );
    b.insert(after("Nf3"), vec![]);
    b
}

fn no_tmp_left(dir: &Path) {
    let names: Vec<_> = fs::read_dir(dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert!(
        names.iter().all(|n| !n.to_string_lossy().contains("tmp")),
        "leftovers: {names:?}"
    );
}

#[test]
fn atomic_write_creates_replaces_and_leaves_no_temporary() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("a.epd");
    let book = cinco();
    book.write_file_atomic(&path, EpdOptions::default())
        .expect("create");
    let back = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
    assert_eq!(back.book, book);
    assert_eq!(back.diagnostics.len(), 0);

    let mut smaller = cinco();
    smaller.strip_opcode("ce");
    smaller
        .write_file_atomic(&path, EpdOptions::default())
        .expect("replace");
    let back = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
    assert_eq!(back.book, smaller);
    assert_eq!(fs::read_dir(dir.path()).expect("dir").count(), 1);
}

#[test]
fn gz_name_writes_compressed_and_scid_dialect_roundtrips() {
    let dir = tempfile::tempdir().expect("tmp");
    for (name, opts) in [
        ("a.epd.gz", EpdOptions::standard()),
        ("b.EPD.GZ", EpdOptions::scid()),
    ] {
        let path = dir.path().join(name);
        let book = cinco();
        book.write_file_atomic(&path, opts).expect("write");
        let raw = fs::read(&path).expect("raw");
        assert_eq!(raw.get(..2), Some(&[0x1f, 0x8b][..]), "gzip magic");
        let back = EpdBook::read_file(&path, opts).expect("read");
        assert_eq!(back.book, book);
    }
    no_tmp_left(dir.path());
}

#[test]
fn unwritable_record_keeps_original_and_leaves_no_temporary() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("keep.epd");
    cinco()
        .write_file_atomic(&path, EpdOptions::default())
        .expect("create");
    let original = fs::read(&path).expect("raw");

    let mut bad = cinco();
    bad.insert(after("g3"), vec![EpdOperation::new("id", "a ; b")]);
    assert!(bad.write_file_atomic(&path, EpdOptions::default()).is_err());
    assert_eq!(fs::read(&path).expect("raw"), original);
    no_tmp_left(dir.path());
}

#[test]
fn missing_directory_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("nope").join("a.epd.gz");
    assert!(
        cinco()
            .write_file_atomic(&path, EpdOptions::default())
            .is_err()
    );
}

#[test]
fn removed_records_close_the_hole_on_write() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("a.epd");
    let mut book = cinco();
    let i = book.index_of(&after("e4")).expect("index");
    assert!(book.remove_at(i).is_some());
    assert!(book.remove_at(i).is_none(), "already removed");
    assert!(!book.contains(&after("e4")));
    assert!(book.is_modified());
    book.write_file_atomic(&path, EpdOptions::default())
        .expect("write");
    let back = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
    assert_eq!(back.book.len(), 4);
    assert_eq!(back.book, book);
    // The position can be added again, at the end.
    book.append_position(&after("e4"));
    assert_eq!(book.len(), 5);
}

#[test]
fn dirty_file_reopens_with_diagnostics_and_survives_a_save() {
    let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";
    let e4 = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -";
    let text = format!(
        "\n# comment line\n{start} bm e4;\nnot a fen at all\n{start} id \"again\";\n\
         {e4} 1x foo;\n{e4} c0 \"open"
    );
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("dirty.epd");
    fs::write(&path, text.as_bytes()).expect("write");

    let load = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
    let kinds: Vec<(u64, &EpdDiagnosticKind)> =
        load.diagnostics.iter().map(|d| (d.line, &d.kind)).collect();
    assert!(
        kinds
            .iter()
            .any(|(l, k)| *l == 4 && matches!(k, EpdDiagnosticKind::InvalidPosition { .. }))
    );
    assert!(kinds.iter().any(
        |(l, k)| *l == 5 && matches!(k, EpdDiagnosticKind::DuplicatePosition { first_line: 3 })
    ));
    assert!(
        kinds
            .iter()
            .any(|(l, k)| *l == 7 && matches!(k, EpdDiagnosticKind::UnterminatedString)),
        "last line without newline was read"
    );
    assert_eq!(load.book.len(), 2);

    // The standard dialect cannot express the unterminated quote: refused,
    // original untouched (the app shows `Unwritable`); the Scid dialect can.
    assert!(
        load.book
            .write_file_atomic(&path, EpdOptions::default())
            .is_err()
    );
    assert_eq!(fs::read(&path).expect("raw"), text.as_bytes());
    load.book
        .write_file_atomic(&path, EpdOptions::scid())
        .expect("save");
    let again = EpdBook::read_file(&path, EpdOptions::scid()).expect("reread");
    assert_eq!(again.book, load.book);
}

#[test]
fn edit_text_then_commit_roundtrip_through_a_file() {
    let mut book = cinco();
    let p = after("c4");
    let rec = book.get_mut(&p).expect("record");
    let warnings = rec.set_operations_text("id \"WAC.001\"\n9x odd\nbm Qxh7+\n");
    assert_eq!(
        warnings,
        vec![TextWarning::NonStandardOpcode {
            line: 2,
            opcode: "9x".into()
        }]
    );
    assert!(book.is_modified());
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("e.epd");
    book.write_file_atomic(&path, EpdOptions::default())
        .expect("write");
    let back = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
    assert_eq!(back.book, book);
    assert_eq!(
        back.book.get(&p).expect("record").operations_text(),
        "id \"WAC.001\"\n9x odd\nbm Qxh7+\n"
    );
}
