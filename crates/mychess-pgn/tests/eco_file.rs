//! Integration tests for reading `.eco` files (`mychess_pgn::eco_file`).
//!
//! Contract: `docs/specs/m1-4-epd-eco.md` (D18/D48, rules 20-27, acceptance
//! criteria for `scid_fr.eco`) and Scid's `PBook::ReadEcoFile` (`pbook.cpp`).

use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use mychess_core::{
    ClassifyOptions, CorruptReason, EcoBookError, EcoDiagnostic, EcoParseOptions, Game, OnError,
    Position, SanMode,
};
use mychess_pgn::eco_file::{EcoFileError, decode_eco_bytes, read_eco_file, read_eco_file_with};
use pretty_assertions::assert_eq;
use proptest::prelude::*;

fn write_tmp(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write file");
    (dir, path)
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(bytes).expect("gzip write");
    gz.finish().expect("gzip finish")
}

// ---------------------------------------------------------------- decoding

#[test]
fn decode_keeps_valid_utf8_untouched() {
    let text = "B20 \"d\u{e9}fense sicilienne \u{2019} T\u{fc}bingen\" 1.e4 c5 *\n";
    assert_eq!(decode_eco_bytes(text.as_bytes()), text);
}

#[test]
fn decode_does_not_mangle_utf8_into_windows_1252() {
    // C3 A9 is `é` in UTF-8; read as cp1252 it would be `Ã©`.
    assert_eq!(decode_eco_bytes(&[b'a', 0xC3, 0xA9]), "a\u{e9}");
}

#[test]
fn decode_falls_back_to_windows_1252_for_invalid_utf8() {
    // 0x92 = U+2019, 0x96 = U+2013, 0xE0 = a-grave, 0xE9 = e-acute.
    let out = decode_eco_bytes(&[0x92, 0x96, 0xE0, 0xE9]);
    assert_eq!(out, "\u{2019}\u{2013}\u{e0}\u{e9}");
}

#[test]
fn decode_windows_1252_covers_all_bytes_of_scid_fr() {
    let bytes = [
        0x85, 0x92, 0x96, 0xE0, 0xE4, 0xE7, 0xE8, 0xE9, 0xEA, 0xEF, 0xF4, 0xF6, 0xFC,
    ];
    assert_eq!(
        decode_eco_bytes(&bytes),
        "\u{2026}\u{2019}\u{2013}\u{e0}\u{e4}\u{e7}\u{e8}\u{e9}\u{ea}\u{ef}\u{f4}\u{f6}\u{fc}"
    );
}

#[test]
fn decode_drops_utf8_bom() {
    assert_eq!(decode_eco_bytes(b"\xEF\xBB\xBFA00 \"x\" *"), "A00 \"x\" *");
}

#[test]
fn decode_drops_bom_before_windows_1252_text() {
    assert_eq!(decode_eco_bytes(b"\xEF\xBB\xBFd\xE9fense"), "d\u{e9}fense");
}

#[test]
fn decode_keeps_bom_only_in_first_position() {
    // A BOM-looking sequence inside the text is data, not a mark.
    assert_eq!(decode_eco_bytes("a\u{feff}b".as_bytes()), "a\u{feff}b");
}

#[test]
fn decode_normalises_crlf_to_lf() {
    assert_eq!(decode_eco_bytes(b"A00\r\nB00\r\n\r\n"), "A00\nB00\n\n");
}

#[test]
fn decode_normalises_crlf_in_windows_1252_text_too() {
    assert_eq!(decode_eco_bytes(b"d\xE9\r\nx"), "d\u{e9}\nx");
}

#[test]
fn decode_empty_and_bom_only_inputs() {
    assert_eq!(decode_eco_bytes(b""), "");
    assert_eq!(decode_eco_bytes(b"\xEF\xBB\xBF"), "");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn decode_never_panics_and_never_leaves_crlf(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let s = decode_eco_bytes(&bytes);
        prop_assert!(!s.contains("\r\n"));
    }
}

// ---------------------------------------------------------------- reading

#[test]
fn reads_plain_utf8_file() {
    let (_d, p) = write_tmp("a.eco", "# c\nA00b \"Barnes\" 1.f3 *\n".as_bytes());
    let r = read_eco_file(&p).expect("read");
    assert_eq!(r.book.len(), 1);
    assert_eq!(r.entries_read, 1);
    assert_eq!(r.book.entries()[0].name(), "Barnes");
}

#[test]
fn reads_windows_1252_file_with_crlf_and_bom() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(b"# comment\r\nB20 \"d\xE9fense sicilienne\" 1.e4 c5 *\r\nB21 \"attaque d\x92Amsterdam\"\r\n 1.e4 c5\r\n 2.f4 *\r\n");
    let (_d, p) = write_tmp("fr.eco", &bytes);
    let r = read_eco_file(&p).expect("read");
    let names: Vec<&str> = r
        .book
        .entries()
        .iter()
        .map(mychess_core::EcoEntry::name)
        .collect();
    assert_eq!(
        names,
        ["d\u{e9}fense sicilienne", "attaque d\u{2019}Amsterdam"]
    );
    // CRLF normalised, so the second entry's `*` is on line 5 of the text.
    assert_eq!(r.book.entries()[1].line(), 5);
    assert_eq!(r.book.entries()[1].moves(), "1.e4 c5 2.f4");
}

#[test]
fn reads_gzip_compressed_file() {
    let (_d, p) = write_tmp("a.eco.gz", &gzip("A00b \"Barnes\" 1.f3 *\r\n".as_bytes()));
    assert_eq!(read_eco_file(&p).expect("read").book.len(), 1);
}

#[test]
fn reads_gzip_of_windows_1252_text() {
    let (_d, p) = write_tmp("fr.eco.gz", &gzip(b"B20 \"d\xE9fense\" 1.e4 c5 *"));
    let r = read_eco_file(&p).expect("read");
    assert_eq!(r.book.entries()[0].name(), "d\u{e9}fense");
}

#[test]
fn gzip_is_detected_by_content_not_by_extension() {
    let (_d, p) = write_tmp("plain_name.eco", &gzip(b"A00b \"Barnes\" 1.f3 *"));
    assert_eq!(read_eco_file(&p).expect("read").book.len(), 1);
}

#[test]
fn reads_empty_file_as_empty_book() {
    let (_d, p) = write_tmp("empty.eco", b"");
    let r = read_eco_file(&p).expect("read");
    assert!(r.book.is_empty());
    assert_eq!(r.entries_read, 0);
}

#[test]
fn accepts_path_and_str_and_pathbuf_arguments() {
    let (_d, p) = write_tmp("a.eco", b"A00b \"Barnes\" 1.f3 *");
    assert!(read_eco_file(&p).is_ok());
    assert!(read_eco_file(p.as_path()).is_ok());
    assert!(read_eco_file(p.to_str().expect("utf8 path")).is_ok());
}

#[test]
fn reports_duplicates_through_the_report() {
    let (_d, p) = write_tmp("d.eco", b"A00b \"1\" 1.f3 *\nA00b \"2\" 1.f3 *\n");
    let r = read_eco_file(&p).expect("read");
    assert_eq!((r.book.len(), r.entries_read), (1, 2));
    assert_eq!(
        r.diagnostics,
        vec![EcoDiagnostic::DuplicatePosition {
            line: 2,
            first_line: 1
        }]
    );
}

// ------------------------------------------------------------------ errors

#[test]
fn missing_file_is_an_io_error_with_the_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("none.eco");
    match read_eco_file(&missing) {
        Err(EcoFileError::Io { path, source }) => {
            assert_eq!(path, missing);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io, got {other:?}"),
    }
}

#[test]
fn directory_instead_of_file_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(read_eco_file(dir.path()).is_err());
}

#[test]
fn corrupt_text_reports_parse_error_with_path_line_and_reason() {
    let (_d, p) = write_tmp("bad.eco", b"A00a \"Start\" *\nB20 \"x\"\n1.e4 e5 2.Ke5 *\n");
    match read_eco_file(&p) {
        Err(EcoFileError::Parse { path, source }) => {
            assert_eq!(path, p);
            match source {
                EcoBookError::Corrupt { line, reason } => {
                    assert_eq!(line, 3);
                    assert!(matches!(reason, CorruptReason::IllegalMove { .. }));
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("expected Parse, got {other:?}"),
    }
}

#[test]
fn line_numbers_count_crlf_files_per_line_not_per_byte_pair() {
    let (_d, p) = write_tmp("bad.eco", b"A00a \"Start\" *\r\n\r\nB2 \"x\" *\r\n");
    match read_eco_file(&p) {
        Err(EcoFileError::Parse {
            source: EcoBookError::Corrupt { line, reason },
            ..
        }) => assert_eq!((line, reason), (3, CorruptReason::BadCode)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn skip_entry_option_is_forwarded_to_the_parser() {
    let (_d, p) = write_tmp(
        "s.eco",
        b"A00b \"a\" 1.f3 *\nB20 \"bad\" 1.zz *\nA00c \"c\" 1.a3 *\n",
    );
    let mut opts = EcoParseOptions::default();
    opts.on_error = OnError::SkipEntry;
    let r = read_eco_file_with(&p, opts).expect("skip");
    assert_eq!(r.book.len(), 2);
    assert!(matches!(
        r.diagnostics[0],
        EcoDiagnostic::EntrySkipped { line: 2, .. }
    ));
    // Default policy aborts on the same file.
    assert!(matches!(read_eco_file(&p), Err(EcoFileError::Parse { .. })));
}

#[test]
fn binary_garbage_file_does_not_panic() {
    let junk: Vec<u8> = (0..4096u32)
        .map(|i| u8::try_from(i * 31 % 256).unwrap_or(0))
        .collect();
    let (_d, p) = write_tmp("junk.eco", &junk);
    drop(read_eco_file(&p));
}

#[test]
fn truncated_gzip_is_an_error_not_a_panic() {
    let mut data = gzip(&b"A00b \"Barnes\" 1.f3 *\n".repeat(200));
    data.truncate(data.len() / 2);
    let (_d, p) = write_tmp("t.eco.gz", &data);
    assert!(read_eco_file(&p).is_err());
}

#[test]
fn bad_gzip_body_after_magic_is_an_error() {
    let (_d, p) = write_tmp("bad.eco.gz", &[0x1F, 0x8B, 0x08, 0x00, 0xFF, 0xFF, 0xFF]);
    assert!(read_eco_file(&p).is_err());
}

#[test]
fn file_over_the_size_limit_is_rejected_as_too_large() {
    // 16 MiB + 1 of blanks: never valid, must be refused by size.
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path().join("big.eco");
    let f = std::fs::File::create(&p).expect("create");
    f.set_len((16 << 20) + 1).expect("sparse");
    match read_eco_file(&p) {
        Err(EcoFileError::TooLarge { path }) => assert_eq!(path, p),
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

#[test]
fn gzip_expanding_beyond_the_limit_is_rejected_as_too_large() {
    // A "gzip bomb": a few KiB on disk, > 16 MiB when expanded.
    let blank = vec![b' '; (16 << 20) + 16];
    let (_d, p) = write_tmp("bomb.eco.gz", &gzip(&blank));
    assert!(matches!(
        read_eco_file(&p),
        Err(EcoFileError::TooLarge { .. })
    ));
}

#[test]
fn file_exactly_at_the_limit_is_not_rejected_for_size() {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path().join("edge.eco");
    let f = std::fs::File::create(&p).expect("create");
    f.set_len(16 << 20).expect("sparse"); // NUL bytes, no entries
    let r = read_eco_file(&p).expect("size is allowed");
    assert!(r.book.is_empty());
}

#[test]
fn error_messages_mention_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let e = read_eco_file(dir.path().join("none.eco")).expect_err("missing");
    assert!(e.to_string().contains("none.eco"), "{e}");
}

// ------------------------------------------------------------------ compat

fn scid_reference() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    dir
}

fn classify_sans(book: &mychess_core::EcoBook, sans: &str) -> Option<(u32, String, String)> {
    let start = Position::new();
    let mut pos = start.clone();
    let mut g = Game::new();
    let mut node = g.root();
    for san in sans.split_whitespace() {
        let mv = pos.parse_san(san, SanMode::Strict).expect("san").mv;
        pos.play(mv).expect("play");
        node = g.add_move(node, mv).expect("add");
    }
    book.classify(&g, ClassifyOptions::game()).map(|m| {
        (
            m.ply,
            m.entry().eco().to_string(),
            m.entry().name().to_owned(),
        )
    })
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_loads_scid_eco_through_read_eco_file() {
    let r = read_eco_file(scid_reference().join("scid.eco")).expect("read");
    assert_eq!(r.entries_read, 10_360);
    assert_eq!(r.book.len(), 10_360);
    assert_eq!(r.diagnostics, vec![]);
    assert_eq!(r.book.fewest_pieces(), 21);
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_loads_scid_fr_eco() {
    let r = read_eco_file(scid_reference().join("scid_fr.eco")).expect("read");
    assert_eq!(r.entries_read, 2_364);
    assert_eq!(r.book.len(), 2_342);
    let dups = r
        .diagnostics
        .iter()
        .filter(|d| matches!(d, EcoDiagnostic::DuplicatePosition { .. }))
        .count();
    assert_eq!(dups, 22);
    assert_eq!(r.diagnostics.len(), 22, "no other diagnostics");
    assert_eq!(r.book.entries()[0].name(), "attaque d\u{2019}Amsterdam");
    assert_eq!(r.book.fewest_pieces(), 22);
    assert!(
        r.book
            .entries()
            .iter()
            .all(|e| e.eco().to_string().len() == 3),
        "scid_fr.eco has only basic codes"
    );
    // No U+FFFD: every 8-bit byte was decoded as windows-1252.
    assert!(
        r.book
            .entries()
            .iter()
            .all(|e| !e.name().contains('\u{fffd}'))
    );
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_scid_fr_spec_vectors() {
    let r = read_eco_file(scid_reference().join("scid_fr.eco")).expect("read");
    let cases = [
        ("e4 c5", 2, "B20", "d\u{e9}fense sicilienne"),
        ("h4 h5", 1, "A00", "ouverture Anti-Borg (= Desprez)"),
        ("Nf3 Nf6 Ng1 Ng8", 2, "A05", "ouverture R\u{e9}ti"),
    ];
    for (sans, ply, code, name) in cases {
        assert_eq!(
            classify_sans(&r.book, sans),
            Some((ply, code.to_owned(), name.to_owned())),
            "{sans}"
        );
    }
    assert!(
        r.book
            .classify(&Game::new(), ClassifyOptions::game())
            .is_none(),
        "scid_fr.eco has no Start entry"
    );
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_scid_fr_lines_with_parentheses_load_without_error() {
    let r = read_eco_file(scid_reference().join("scid_fr.eco")).expect("read");
    let odd = r
        .book
        .entries()
        .iter()
        .filter(|e| {
            e.moves().contains('(')
                || e.moves()
                    .split_whitespace()
                    .any(|w| w.chars().all(|c| c.is_ascii_digit()))
        })
        .count();
    assert!(
        odd >= 1,
        "expected entries with text outside pure SAN, found {odd}"
    );
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_scid_eco_round_trips_through_text_rewrite() {
    // Rewrite every entry as `code "name" moves *` and reload: same positions.
    let r = read_eco_file(scid_reference().join("scid.eco")).expect("read");
    let mut text = String::new();
    for e in r.book.entries() {
        text.extend([
            e.eco().to_string().as_str(),
            " \"",
            e.name(),
            "\" ",
            e.moves(),
            " *\n",
        ]);
    }
    let r2 = mychess_core::EcoBook::parse(&text).expect("reparse");
    assert_eq!(r2.len(), r.book.len());
    for (a, b) in r.book.entries().iter().zip(r2.entries()) {
        assert_eq!(a.eco(), b.eco());
        assert_eq!(a.name(), b.name());
        assert_eq!(a.position(), b.position());
    }
}
