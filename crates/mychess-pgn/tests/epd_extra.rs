//! Extra M7-5a tests derived from `docs/specs/m7-5-janela-epd.md`
//! (D3800-D3849): text editing of records, text search, atomic and gzip
//! writing, hostile inputs, large books and property tests.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use mychess_core::{Position, SanMode};
use mychess_pgn::epd::{
    EpdBook, EpdDiagnosticKind, EpdError, EpdOperation, EpdOptions, EpdRecord, TextWarning,
};
use proptest::prelude::*;

// ---------------------------------------------------------------- helpers

fn after(sans: &str) -> Position {
    let mut p = Position::new();
    for s in sans.split_whitespace() {
        let m = p.parse_san(s, SanMode::Strict).expect("san").mv;
        p = p.after(m).expect("legal");
    }
    p
}

fn record() -> EpdRecord {
    EpdRecord::new(Position::new())
}

fn pairs(r: &EpdRecord) -> Vec<(String, String)> {
    r.operations()
        .iter()
        .map(|o| (o.opcode.clone(), o.operands.clone()))
        .collect()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

const START4: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";
const E4_4: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -";

fn book_with_ids(ids: &[&str]) -> EpdBook {
    let mut book = EpdBook::new();
    let sans = ["", "e4", "d4", "c4", "Nf3", "g3", "b3", "f4"];
    for (i, id) in ids.iter().enumerate() {
        book.insert(
            after(sans[i % sans.len()]),
            vec![EpdOperation::new("id", format!("\"{id}\""))],
        );
    }
    book
}

// ------------------------------------------------------- operations text

#[test]
fn text_accepts_every_line_break_style_and_no_final_newline() {
    for text in [
        "bm e4\nce 25",
        "bm e4\r\nce 25\r\n",
        "bm e4\rce 25\r",
        "bm e4\n\rce 25",
        "bm e4\r\n\r\nce 25",
    ] {
        let mut r = record();
        assert!(r.set_operations_text(text).is_empty(), "{text:?}");
        assert_eq!(
            pairs(&r),
            vec![
                ("bm".to_owned(), "e4".to_owned()),
                ("ce".to_owned(), "25".to_owned())
            ],
            "{text:?}"
        );
    }
}

#[test]
fn text_with_tabs_and_unicode_whitespace_splits_opcode_and_operands() {
    let mut r = record();
    r.set_operations_text("bm\tQxh7+\nc0\u{a0}\u{a0}Zürich 日本\u{a0}\n\u{3000}ce 25");
    assert_eq!(
        pairs(&r),
        vec![
            ("bm".to_owned(), "Qxh7+".to_owned()),
            ("c0".to_owned(), "Zürich 日本".to_owned()),
            ("ce".to_owned(), "25".to_owned()),
        ]
    );
}

#[test]
fn warning_lines_count_blank_lines_and_all_line_break_styles() {
    let mut r = record();
    let w = r.set_operations_text("bm e4\r\n\r\n1x a\rok\n_u b\n\u{e9}t c");
    let lines: Vec<(usize, &str)> = w
        .iter()
        .filter_map(|w| match w {
            TextWarning::NonStandardOpcode { line, opcode } => Some((*line, opcode.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(lines, vec![(3, "1x"), (5, "_u"), (6, "\u{e9}t")]);
    assert_eq!(r.operations().len(), 6 - 1, "blank line dropped, rest kept");
}

#[test]
fn opcode_length_boundary_is_fourteen_characters() {
    let mut r = record();
    let ok = "a".repeat(14);
    let bad = "a".repeat(15);
    let w = r.set_operations_text(&format!("{ok} x\n{bad} y"));
    assert_eq!(w.len(), 1);
    assert_eq!(r.operations().len(), 2);
}

#[test]
fn trailing_semicolon_in_the_editor_is_refused_in_standard_and_escaped_in_scid() {
    let mut book = EpdBook::new();
    book.insert(Position::new(), vec![]);
    let pos = Position::new();
    book.get_mut(&pos)
        .expect("rec")
        .set_operations_text("bm e4;\n");
    let mut buf = Vec::new();
    assert!(matches!(
        book.write(&mut buf, EpdOptions::standard()),
        Err(EpdError::Unwritable { .. })
    ));
    assert!(buf.is_empty(), "nothing written on Unwritable");

    book.write(&mut buf, EpdOptions::scid()).expect("scid");
    let back = EpdBook::read(buf.as_slice(), EpdOptions::scid())
        .expect("read")
        .book;
    assert_eq!(back, book);
    assert_eq!(back.get(&pos).expect("rec").operations_text(), "bm e4;\n");
}

#[test]
fn escaped_quotes_and_backslashes_roundtrip_in_both_dialects() {
    let tricky = [
        r#"c0 "he said \"hi\"""#,
        r#"c1 "a\\b""#,
        r#"c2 "semi; colon \s ok""#,
        r"c3 C:\dir\file",
    ];
    let text = tricky.join("\n");
    let mut r = record();
    r.set_operations_text(&text);
    let pos = Position::new();
    for opts in [EpdOptions::standard(), EpdOptions::scid()] {
        let mut book = EpdBook::new();
        book.insert(pos.clone(), r.operations().to_vec());
        let mut buf = Vec::new();
        book.write(&mut buf, opts).expect("write");
        let back = EpdBook::read(buf.as_slice(), opts).expect("read");
        assert!(back.diagnostics.is_empty(), "{:?}", back.diagnostics);
        assert_eq!(back.book, book);
        assert_eq!(
            back.book.get(&pos).expect("rec").operations_text(),
            format!("{text}\n")
        );
    }
}

#[test]
fn semicolon_after_an_escaped_quote_is_unwritable_in_standard_dialect() {
    // `"a\";b"`: the standard dialect has no escapes, the quote after the
    // backslash closes the string, so the `;` is a separator.
    let mut r = record();
    r.set_operations_text(r#"c0 "a\";b""#);
    let mut book = EpdBook::new();
    book.insert(Position::new(), r.operations().to_vec());
    let mut buf = Vec::new();
    assert!(book.write(&mut buf, EpdOptions::standard()).is_err());
    book.write(&mut buf, EpdOptions::scid()).expect("scid ok");
    let back = EpdBook::read(buf.as_slice(), EpdOptions::scid())
        .expect("read")
        .book;
    assert_eq!(back, book);
}

#[test]
fn text_of_a_hundred_thousand_lines_is_linear() {
    let mut text = String::new();
    for i in 0..100_000 {
        let _ = writeln!(text, "c{} v{i}", i % 7);
    }
    let mut r = record();
    let t = Instant::now();
    let w = r.set_operations_text(&text);
    assert_eq!(w.len(), 0);
    assert_eq!(r.operations().len(), 100_000);
    assert_eq!(r.operations_text(), text);
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
}

fn operand_strategy() -> impl Strategy<Value = String> {
    // Any text without line breaks; trimmed like `EpdOperation::new` does.
    "[^\r\n]{0,24}".prop_map(|s| s.trim().to_owned())
}

fn opcode_strategy() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9_]{0,13}"
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        cases: 200,
        ..ProptestConfig::default()
    })]

    #[test]
    fn operations_text_roundtrips_any_single_line_operands(
        ops in prop::collection::vec((opcode_strategy(), operand_strategy()), 0..8)
    ) {
        let ops: Vec<EpdOperation> = ops.into_iter().map(|(c, o)| EpdOperation::new(c, o)).collect();
        let rec = EpdRecord::with_operations(Position::new(), ops.clone());
        let text = rec.operations_text();
        let mut back = record();
        let warnings = back.set_operations_text(&text);
        prop_assert!(warnings.is_empty(), "{:?}", warnings);
        prop_assert_eq!(back.operations(), &ops[..]);
        // Reparsing the text is a fixed point.
        prop_assert_eq!(back.operations_text(), text);
    }

    #[test]
    fn set_operations_text_never_panics_on_arbitrary_text(text in "\\PC{0,200}|[\\x00-\\x7f]{0,200}") {
        let mut r = record();
        drop(r.set_operations_text(&text));
        // Whatever was parsed prints and parses to the same operations.
        let printed = r.operations_text();
        let mut again = record();
        again.set_operations_text(&printed);
        prop_assert_eq!(again.operations(), r.operations());
    }
}

// Operands: segments of plain text and quoted text; quoted text may hold
// `;` and `\`. Valid in the standard dialect (balanced quotes, `;` only
// inside) and, escaped by the writer, in the Scid dialect.
fn segmented_operand() -> impl Strategy<Value = String> {
    let plain = "[A-Za-z0-9 .+#=!?/\\-]{0,6}";
    let quoted = "[A-Za-z0-9 ;\\\\.]{0,6}".prop_map(|s| format!("\"{s}\""));
    prop::collection::vec(prop_oneof![plain.prop_map(String::from), quoted], 0..4)
        .prop_map(|v| v.concat().trim().to_owned())
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        cases: 200,
        ..ProptestConfig::default()
    })]

    #[test]
    fn books_roundtrip_through_write_and_read_in_both_dialects(
        ops in prop::collection::vec((opcode_strategy(), segmented_operand()), 0..6),
        scid in any::<bool>(),
    ) {
        let opts = if scid { EpdOptions::scid() } else { EpdOptions::standard() };
        let ops: Vec<EpdOperation> = ops.into_iter().map(|(c, o)| EpdOperation::new(c, o)).collect();
        let mut book = EpdBook::new();
        book.insert(after("e4"), ops);
        book.insert(Position::new(), vec![EpdOperation::new("id", "\"x;y\"")]);
        let mut buf = Vec::new();
        book.write(&mut buf, opts).expect("write");
        prop_assert!(!buf.contains(&b'\r'), "output uses \\n only");
        let back = EpdBook::read(buf.as_slice(), opts).expect("read");
        prop_assert!(back.diagnostics.iter().all(|d| matches!(d.kind, EpdDiagnosticKind::NonStandardOpcode{..}) ), "{:?}", back.diagnostics);
        prop_assert_eq!(back.book, book);
    }
}

// ---------------------------------------------------------------- find_text

#[test]
fn find_text_unicode_special_casing_is_safe_and_documented() {
    // D3829: per-character folding; characters whose lowercase is not a
    // single character (U+0130) are compared as they are.
    let mut book = EpdBook::new();
    book.insert(
        Position::new(),
        vec![EpdOperation::new("c0", "\"İstanbul\"")],
    );
    book.insert(after("e4"), vec![EpdOperation::new("c0", "\"Straße\"")]);
    book.insert(after("d4"), vec![EpdOperation::new("c0", "\"ǅemal\"")]);

    let slice = |book: &EpdBook, needle: &str, forward: bool| -> Option<String> {
        let (i, r) = book.find_text(None, needle, forward)?;
        let text = book.get_by_index(i)?.search_text();
        Some(text.get(r)?.to_owned())
    };
    // Exact characters are found, with the range on char boundaries.
    assert_eq!(slice(&book, "İstanbul", true).as_deref(), Some("İstanbul"));
    assert_eq!(slice(&book, "STRAßE", true).as_deref(), Some("Straße"));
    assert_eq!(slice(&book, "ß", false).as_deref(), Some("ß"));
    // U+01C5 folds to U+01C6 on both sides.
    assert_eq!(slice(&book, "ǆemal", true).as_deref(), Some("ǅemal"));
    assert_eq!(slice(&book, "Ǆ", true).as_deref(), Some("ǅ"));
    // Documented non-matches.
    assert_eq!(book.find_text(None, "istanbul", true), None, "i != İ");
    assert_eq!(book.find_text(None, "strasse", true), None, "ss != ß");
    assert_eq!(book.find_text(None, "ss", true), None);
}

#[test]
fn find_text_ranges_are_valid_for_arbitrary_unicode_haystacks() {
    let mut book = EpdBook::new();
    book.insert(
        Position::new(),
        vec![EpdOperation::new("c0", "\"日本語 ÄÖÜ ǅ İİ ß ﬃ 😀 end\"")],
    );
    for needle in [
        "日本", "äöü", "😀", "ﬃ", "İİ", "end", "c0", "\n", "kq -\nc0", " ",
    ] {
        if let Some((i, r)) = book.find_text(None, needle, true) {
            let text = book.get_by_index(i).expect("rec").search_text();
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
            assert!(text.get(r.clone()).is_some(), "{needle:?}");
        } else {
            panic!("needle {needle:?} should match");
        }
    }
}

#[test]
fn find_text_matches_fen_and_opcodes_without_regard_to_case() {
    let book = book_with_ids(&["WAC.001", "WAC.002", "OTHER"]);
    let (i0, r) = book.find_text(None, "RNBQKBNR W KQKQ", true).expect("fen");
    let text = book.get_by_index(i0).expect("rec").search_text();
    assert_eq!(text[r].to_lowercase(), "rnbqkbnr w kqkq");
    for needle in ["id \"wac.002\"", "ID \"WAC.002\"", "wac.002"] {
        let (i, _) = book.find_text(None, needle, true).expect(needle);
        let text = book.get_by_index(i).expect("rec").search_text();
        assert!(text.to_lowercase().contains("wac.002"));
    }
    assert!(book.find_text(None, "nothing here", true).is_none());
    assert!(book.find_text(None, "", true).is_none());
    assert!(EpdBook::new().find_text(None, "x", true).is_none());
}

#[test]
fn find_text_walks_forward_and_backward_with_wraparound() {
    let book = book_with_ids(&["A1", "B2", "A3", "B4"]);
    let idx: Vec<_> = book.iter_indexed().map(|(i, _)| i).collect();
    // Forward from None: first match; then successive calls visit all and wrap.
    let mut seen = Vec::new();
    let mut from = None;
    for _ in 0..5 {
        let (i, _) = book.find_text(from, "\"a", true).expect("a");
        seen.push(i);
        from = Some(i);
    }
    assert_eq!(seen, vec![idx[0], idx[2], idx[0], idx[2], idx[0]]);
    // Backward from None: starts at the last record.
    let (i, _) = book.find_text(None, "\"b", false).expect("b");
    assert_eq!(i, idx[3]);
    let (i, _) = book.find_text(Some(idx[3]), "\"b", false).expect("b");
    assert_eq!(i, idx[1]);
    let (i, _) = book.find_text(Some(idx[1]), "\"b", false).expect("b");
    assert_eq!(i, idx[3], "wraps backwards");
    // `from` is the last record examined: a single match finds itself.
    let (i, _) = book.find_text(Some(idx[1]), "b2", true).expect("self");
    assert_eq!(i, idx[1]);
    let (i, _) = book.find_text(Some(idx[1]), "b2", false).expect("self");
    assert_eq!(i, idx[1]);
}

#[test]
fn find_text_skips_removed_records_and_survives_a_removed_start() {
    let mut book = book_with_ids(&["A1", "B2", "A3"]);
    let idx: Vec<_> = book.iter_indexed().map(|(i, _)| i).collect();
    book.remove_at(idx[1]);
    let (i, _) = book.find_text(Some(idx[0]), "\"a", true).expect("a");
    assert_eq!(i, idx[2]);
    assert!(
        book.find_text(None, "b2", true).is_none(),
        "removed text is gone"
    );
    // `from` pointing to a removed record must not panic; it restarts.
    let found = book.find_text(Some(idx[1]), "\"a", true);
    assert!(found.is_some());
    let found = book.find_text(Some(idx[1]), "a", false);
    assert!(found.is_some());
    // Everything removed.
    book.remove_at(idx[0]);
    book.remove_at(idx[2]);
    assert!(book.find_text(None, "\"a", true).is_none());
    assert!(book.find_text(Some(idx[0]), "a", false).is_none());
}

#[test]
fn find_text_needle_larger_than_any_record_or_with_line_breaks_does_not_panic() {
    let book = book_with_ids(&["A"]);
    assert!(book.find_text(None, &"x".repeat(100_000), true).is_none());
    assert!(book.find_text(None, "\r\n", true).is_none());
    // The record separator is `\n` between FEN and operations.
    assert!(book.find_text(None, "kq -\nid", true).is_some());
}

// ------------------------------------------------------- atomic writing

#[test]
fn empty_book_writes_an_empty_file_and_an_empty_gzip_that_reads_back() {
    let dir = tempfile::tempdir().expect("tmp");
    let plain = dir.path().join("e.epd");
    let gz = dir.path().join("e.epd.gz");
    EpdBook::new()
        .write_file_atomic(&plain, EpdOptions::default())
        .expect("plain");
    EpdBook::new()
        .write_file_atomic(&gz, EpdOptions::default())
        .expect("gz");
    assert_eq!(fs::read(&plain).expect("raw").len(), 0);
    assert_eq!(
        fs::read(&gz).expect("raw").get(..2),
        Some(&[0x1f, 0x8b][..])
    );
    for p in [&plain, &gz] {
        let back = EpdBook::read_file(p, EpdOptions::default()).expect("read");
        assert!(back.book.is_empty() && back.diagnostics.is_empty());
    }
    assert_eq!(names(dir.path()), vec!["e.epd", "e.epd.gz"]);
}

#[test]
fn write_file_refuses_gz_and_creates_nothing() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("x.epd.GZ");
    let err = book_with_ids(&["A"])
        .write_file(&path, EpdOptions::default())
        .expect_err("refused");
    assert!(matches!(err, EpdError::Unsupported { .. }));
    assert_eq!(names(dir.path()).len(), 0);
}

#[test]
fn gz_decision_uses_only_the_final_extension() {
    let dir = tempfile::tempdir().expect("tmp");
    let book = book_with_ids(&["A"]);
    for (name, gz) in [
        ("a.gz.epd", false),
        ("a.epd.gz.bak", false),
        ("a.epd.Gz", true),
        ("noext", false),
    ] {
        let p = dir.path().join(name);
        book.write_file_atomic(&p, EpdOptions::default())
            .expect(name);
        let raw = fs::read(&p).expect("raw");
        assert_eq!(raw.starts_with(&[0x1f, 0x8b]), gz, "{name}");
        let back = EpdBook::read_file(&p, EpdOptions::default()).expect("read");
        assert_eq!(back.book, book, "{name}");
    }
}

#[test]
fn unwritable_record_in_the_middle_keeps_a_gz_original_and_no_temporary() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("keep.epd.gz");
    let good = book_with_ids(&["A", "B", "C"]);
    good.write_file_atomic(&path, EpdOptions::default())
        .expect("create");
    let original = fs::read(&path).expect("raw");

    let mut bad = book_with_ids(&["A", "B", "C"]);
    let pos = after("d4");
    bad.get_mut(&pos)
        .expect("rec")
        .set_operations_text("id \"unterminated");
    let err = bad
        .write_file_atomic(&path, EpdOptions::default())
        .expect_err("unwritable");
    assert!(matches!(err, EpdError::Unwritable { .. }), "{err:?}");
    assert_eq!(fs::read(&path).expect("raw"), original);
    assert_eq!(names(dir.path()), vec!["keep.epd.gz"]);
}

#[test]
fn rename_failure_because_target_is_a_directory_leaves_no_temporary() {
    let dir = tempfile::tempdir().expect("tmp");
    for name in ["d.epd", "d.epd.gz"] {
        let target = dir.path().join(name);
        fs::create_dir(&target).expect("mkdir");
        let r = book_with_ids(&["A"]).write_file_atomic(&target, EpdOptions::default());
        assert!(r.is_err(), "{name}");
        assert!(target.is_dir(), "{name} must still be a directory");
        assert_eq!(names(dir.path()).len(), 1, "{name}: no temporary left");
        fs::remove_dir(&target).expect("rmdir");
    }
    assert!(
        names(dir.path()).is_empty(),
        "no .tmp left: {:?}",
        names(dir.path())
    );
}

#[test]
fn paths_without_file_name_or_parent_are_errors_not_panics() {
    let dir = tempfile::tempdir().expect("tmp");
    let book = book_with_ids(&["A"]);
    assert!(
        book.write_file_atomic(Path::new(""), EpdOptions::default())
            .is_err()
    );
    assert!(
        book.write_file_atomic(dir.path().join(".."), EpdOptions::default())
            .is_err()
    );
    assert!(
        book.write_file_atomic(dir.path(), EpdOptions::default())
            .is_err()
    );
    assert!(
        book.write_file_atomic(dir.path().join("a").join("b.epd"), EpdOptions::default())
            .is_err()
    );
    assert_eq!(names(dir.path()).len(), 0);
}

#[test]
fn saving_twice_is_stable_and_leaves_one_file() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("s.epd");
    let book = book_with_ids(&["A", "B"]);
    book.write_file_atomic(&path, EpdOptions::default())
        .expect("1");
    let first = fs::read(&path).expect("raw");
    book.write_file_atomic(&path, EpdOptions::default())
        .expect("2");
    assert_eq!(fs::read(&path).expect("raw"), first);
    assert!(!first.contains(&b'\r'));
    assert_eq!(names(dir.path()), vec!["s.epd"]);
}

#[cfg(unix)]
#[test]
fn read_only_destination_either_replaces_or_fails_cleanly() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("ro.epd");
    book_with_ids(&["OLD"])
        .write_file_atomic(&path, EpdOptions::default())
        .expect("create");
    let original = fs::read(&path).expect("raw");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("chmod");

    let newer = book_with_ids(&["NEW", "NEWER"]);
    match newer.write_file_atomic(&path, EpdOptions::default()) {
        Ok(()) => {
            // Unix `rename` replaces a read-only file when the directory is
            // writable; the permissions are kept.
            let back = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
            assert_eq!(back.book, newer);
            let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o444, "permissions of the target are preserved");
        }
        Err(_) => assert_eq!(fs::read(&path).expect("raw"), original),
    }
    assert_eq!(names(dir.path()), vec!["ro.epd"]);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");
}

#[cfg(unix)]
#[test]
fn read_only_directory_fails_cleanly_and_keeps_the_original() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tmp");
    let sub = dir.path().join("ro");
    fs::create_dir(&sub).expect("mkdir");
    let path = sub.join("a.epd");
    book_with_ids(&["OLD"])
        .write_file_atomic(&path, EpdOptions::default())
        .expect("create");
    let original = fs::read(&path).expect("raw");
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).expect("chmod");
    let probe = fs::write(sub.join("probe"), b"x");
    if probe.is_ok() {
        // Running as root: permissions are not enforced; nothing to test.
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).expect("chmod");
        return;
    }
    let r = book_with_ids(&["NEW"]).write_file_atomic(&path, EpdOptions::default());
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).expect("chmod");
    assert!(r.is_err());
    assert_eq!(fs::read(&path).expect("raw"), original);
    assert_eq!(names(&sub), vec!["a.epd"]);
}

#[cfg(unix)]
#[test]
fn symlink_targets_are_replaced_and_the_link_is_kept_dangling_links_fail() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("tmp");
    let real = dir.path().join("real.epd");
    let link = dir.path().join("link.epd");
    book_with_ids(&["OLD"])
        .write_file_atomic(&real, EpdOptions::default())
        .expect("create");
    symlink(&real, &link).expect("symlink");
    let newer = book_with_ids(&["NEW"]);
    newer
        .write_file_atomic(&link, EpdOptions::default())
        .expect("via link");
    assert!(
        fs::symlink_metadata(&link)
            .expect("meta")
            .file_type()
            .is_symlink()
    );
    let back = EpdBook::read_file(&real, EpdOptions::default()).expect("read");
    assert_eq!(back.book, newer);

    let dangling = dir.path().join("dangling.epd");
    symlink(dir.path().join("missing").join("x.epd"), &dangling).expect("symlink");
    assert!(
        newer
            .write_file_atomic(&dangling, EpdOptions::default())
            .is_err()
    );
    assert_eq!(
        names(dir.path()),
        vec!["dangling.epd", "link.epd", "real.epd"]
    );
}

// ------------------------------------------------- hostile read inputs

#[test]
fn bom_crlf_and_cr_only_files_read_the_same_book() {
    let dir = tempfile::tempdir().expect("tmp");
    let lines = [format!("{START4} bm e4;"), format!("{E4_4} id \"x\";")];
    let variants: [(&str, Vec<u8>); 4] = [
        ("lf", format!("{}\n{}\n", lines[0], lines[1]).into_bytes()),
        (
            "crlf",
            format!("{}\r\n{}\r\n", lines[0], lines[1]).into_bytes(),
        ),
        ("cr", format!("{}\r{}", lines[0], lines[1]).into_bytes()),
        (
            "bom",
            [
                b"\xEF\xBB\xBF".as_slice(),
                format!("{}\n{}", lines[0], lines[1]).as_bytes(),
            ]
            .concat(),
        ),
    ];
    let mut books = Vec::new();
    for (name, bytes) in variants {
        let p = dir.path().join(format!("{name}.epd"));
        fs::write(&p, bytes).expect("write");
        let load = EpdBook::read_file(&p, EpdOptions::default()).expect("read");
        assert!(
            load.diagnostics.is_empty(),
            "{name}: {:?}",
            load.diagnostics
        );
        assert_eq!(load.book.len(), 2, "{name}");
        books.push(load.book);
    }
    assert!(books.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn latin1_and_invalid_utf8_are_decoded_and_saved_as_utf8() {
    let dir = tempfile::tempdir().expect("tmp");
    let p = dir.path().join("l1.epd");
    let mut bytes = format!("{START4} c0 \"Caf").into_bytes();
    bytes.push(0xE9); // é in Latin-1 / windows-1252
    bytes.extend_from_slice(b"\";\n");
    bytes.extend_from_slice(format!("{E4_4} c0 \"bad ").as_bytes());
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    bytes.extend_from_slice(b"\";\n");
    fs::write(&p, &bytes).expect("write");
    let load = EpdBook::read_file(&p, EpdOptions::default()).expect("read");
    assert_eq!(load.book.len(), 2);
    let rec = load.book.get(&Position::new()).expect("rec");
    assert_eq!(rec.operation("c0").expect("op").operands, "\"Café\"");

    let out = dir.path().join("out.epd");
    load.book
        .write_file_atomic(&out, EpdOptions::default())
        .expect("save");
    let raw = fs::read(&out).expect("raw");
    assert!(std::str::from_utf8(&raw).is_ok(), "saved file is UTF-8");
    let back = EpdBook::read_file(&out, EpdOptions::default()).expect("read");
    assert_eq!(back.book, load.book);
}

#[test]
fn empty_and_garbage_only_files_give_empty_books_with_diagnostics() {
    let dir = tempfile::tempdir().expect("tmp");
    let empty = dir.path().join("empty.epd");
    fs::write(&empty, b"").expect("write");
    let load = EpdBook::read_file(&empty, EpdOptions::default()).expect("read");
    assert!(load.book.is_empty() && load.diagnostics.is_empty());

    let junk = dir.path().join("junk.epd");
    fs::write(&junk, [0u8, 1, 2, 3, 255, 254, b'\n', 0, 0, b'\r']).expect("write");
    let load = EpdBook::read_file(&junk, EpdOptions::default()).expect("read");
    assert!(load.book.is_empty());
    assert_ne!(load.diagnostics.len(), 0);
}

#[test]
fn truncated_or_corrupt_gzip_never_panics() {
    let dir = tempfile::tempdir().expect("tmp");
    let good = dir.path().join("g.epd.gz");
    book_with_ids(&["A", "B", "C", "D"])
        .write_file_atomic(&good, EpdOptions::default())
        .expect("write");
    let raw = fs::read(&good).expect("raw");
    for cut in [2, 5, 10, raw.len() / 2, raw.len() - 1] {
        let p = dir.path().join(format!("cut{cut}.epd"));
        fs::write(&p, &raw[..cut]).expect("write");
        // Err or a partial book are both acceptable; panics are not.
        drop(EpdBook::read_file(&p, EpdOptions::default()));
    }
    let mut flipped = raw.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 0xFF;
    let p = dir.path().join("flip.epd");
    fs::write(&p, flipped).expect("write");
    drop(EpdBook::read_file(&p, EpdOptions::default()));
}

#[test]
fn huge_line_is_reported_not_loaded_and_neighbours_survive() {
    let dir = tempfile::tempdir().expect("tmp");
    let p = dir.path().join("huge.epd");
    let huge = format!("{START4} c0 \"{}\";", "x".repeat(2 << 20));
    fs::write(&p, format!("{huge}\n{E4_4} bm e5;\n{huge}")).expect("write");
    let load = EpdBook::read_file(&p, EpdOptions::default()).expect("read");
    let kinds: Vec<(u64, &EpdDiagnosticKind)> =
        load.diagnostics.iter().map(|d| (d.line, &d.kind)).collect();
    assert!(
        kinds
            .iter()
            .any(|(l, k)| *l == 1 && **k == EpdDiagnosticKind::LineTooLong)
    );
    assert!(
        kinds
            .iter()
            .any(|(l, k)| *l == 3 && **k == EpdDiagnosticKind::LineTooLong)
    );
    assert_eq!(load.book.len(), 1);
}

#[test]
fn a_record_with_a_huge_operand_is_refused_on_save() {
    let dir = tempfile::tempdir().expect("tmp");
    let p = dir.path().join("big.epd");
    let mut book = EpdBook::new();
    book.insert(
        Position::new(),
        vec![EpdOperation::new(
            "c0",
            format!("\"{}\"", "x".repeat(1_200_000)),
        )],
    );
    // D3831: a line over the reader's limit is refused, never half-saved.
    let saved = book.write_file_atomic(&p, EpdOptions::default());
    assert!(
        matches!(saved, Err(EpdError::Unwritable { .. })),
        "{saved:?}"
    );
    assert!(!p.exists(), "nothing is created");
}

// --------------------------------------------------------- big books

/// Valid, distinct positions: black king, white king and white rook on three
/// squares (kings apart, rook off the black king's lines).
fn big_epd(count: usize) -> String {
    let mut out = String::with_capacity(count * 40);
    let sq = |i: usize| (i / 8, i % 8);
    let mut n = 0;
    'outer: for bk in 0..64 {
        for wk in 0..64 {
            for wr in 0..64 {
                let (bk_r, bk_f) = sq(bk);
                let (wk_r, wk_f) = sq(wk);
                let (wr_r, wr_f) = sq(wr);
                let apart = bk_r.abs_diff(wk_r) > 1 || bk_f.abs_diff(wk_f) > 1;
                if bk == wk || bk == wr || wk == wr || !apart || wr_r == bk_r || wr_f == bk_f {
                    continue;
                }
                let mut board = [[b'1'; 8]; 8];
                board[bk_r][bk_f] = b'k';
                board[wk_r][wk_f] = b'K';
                board[wr_r][wr_f] = b'R';
                let rows: Vec<String> = board
                    .iter()
                    .map(|row| {
                        let mut s = String::new();
                        let mut empty = 0;
                        for &c in row {
                            if c == b'1' {
                                empty += 1;
                            } else {
                                if empty > 0 {
                                    s.push_str(&empty.to_string());
                                    empty = 0;
                                }
                                s.push(char::from(c));
                            }
                        }
                        if empty > 0 {
                            s.push_str(&empty.to_string());
                        }
                        s
                    })
                    .collect();
                let _ = writeln!(out, "{} w - - id \"P{n}\"; ce {};", rows.join("/"), n % 500);
                n += 1;
                if n == count {
                    break 'outer;
                }
            }
        }
    }
    assert_eq!(n, count, "generator produced too few positions");
    out
}

#[test]
fn hundred_thousand_records_read_search_remove_and_write_in_reasonable_time() {
    let limit = Duration::from_secs(60);
    let t0 = Instant::now();
    let text = big_epd(100_000);
    let load = EpdBook::read(text.as_bytes(), EpdOptions::default()).expect("read");
    let mut book = load.book;
    assert!(
        load.diagnostics.is_empty(),
        "{:?}",
        &load.diagnostics[..load.diagnostics.len().min(3)]
    );
    assert_eq!(book.len(), 100_000);
    let read_time = t0.elapsed();

    // A miss scans every record exactly once.
    let t = Instant::now();
    assert!(book.find_text(None, "no-such-text", true).is_none());
    assert!(book.find_text(None, "no-such-text", false).is_none());
    let miss_time = t.elapsed();

    // Hits at the very end, from the start and backwards.
    let (last, _) = book.find_text(None, "id \"P99999\"", true).expect("last");
    assert_eq!(Some(last), book.last_index());
    let (first, _) = book.find_text(None, "id \"P0\"", false).expect("first");
    assert_eq!(Some(first), book.iter_indexed().next().map(|(i, _)| i));

    // Remove every other record by handle, then a search must still be fast.
    let t = Instant::now();
    let handles: Vec<_> = book.iter_indexed().map(|(i, _)| i).collect();
    for (n, h) in handles.iter().enumerate() {
        if n % 2 == 0 {
            assert!(book.remove_at(*h).is_some());
        }
    }
    assert_eq!(book.len(), 50_000);
    assert!(book.find_text(None, "no-such-text", true).is_none());
    let (hit, _) = book
        .find_text(None, "id \"P99999\"", true)
        .expect("still there");
    assert_eq!(Some(hit), book.last_index());
    let remove_time = t.elapsed();

    // Remove all but the last: next/prev over a long run of holes.
    let t = Instant::now();
    for h in handles.iter().take(handles.len() - 1) {
        book.remove_at(*h);
    }
    assert_eq!(book.len(), 1);
    for _ in 0..200 {
        let (i, _) = book.find_text(None, "p99999", true).expect("hit");
        assert_eq!(Some(i), book.last_index());
        assert!(book.find_text(Some(i), "p99999", false).is_some());
    }
    let holes_time = t.elapsed();

    // Write and reread what is left of the big book, plain and gz.
    let big = EpdBook::read(text.as_bytes(), EpdOptions::default())
        .expect("read")
        .book;
    let dir = tempfile::tempdir().expect("tmp");
    let t = Instant::now();
    for name in ["big.epd", "big.epd.gz"] {
        let p = dir.path().join(name);
        big.write_file_atomic(&p, EpdOptions::default())
            .expect("write");
        let back = EpdBook::read_file(&p, EpdOptions::default()).expect("read");
        assert_eq!(back.book, big, "{name}");
    }
    let write_time = t.elapsed();

    eprintln!(
        "read {read_time:?}, miss x2 {miss_time:?}, remove {remove_time:?}, holes {holes_time:?}, write {write_time:?}"
    );
    assert!(t0.elapsed() < limit, "total {:?}", t0.elapsed());
    assert!(miss_time < Duration::from_secs(20), "miss {miss_time:?}");
    assert!(holes_time < Duration::from_secs(20), "holes {holes_time:?}");
}

#[test]
fn removed_holes_are_not_written_and_compact_keeps_order() {
    let dir = tempfile::tempdir().expect("tmp");
    let mut book = book_with_ids(&["A", "B", "C", "D", "E"]);
    let idx: Vec<_> = book.iter_indexed().map(|(i, _)| i).collect();
    book.remove_at(idx[0]);
    book.remove_at(idx[2]);
    book.remove_at(idx[4]);
    let p = dir.path().join("h.epd");
    book.write_file_atomic(&p, EpdOptions::default())
        .expect("write");
    let before: Vec<String> = book.iter().map(EpdRecord::operations_text).collect();
    book.compact();
    let after_compact: Vec<String> = book.iter().map(EpdRecord::operations_text).collect();
    assert_eq!(before, after_compact);
    let back = EpdBook::read_file(&p, EpdOptions::default()).expect("read");
    assert_eq!(back.book, book);
    assert_eq!(back.book.len(), 2);
}
