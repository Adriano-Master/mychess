//! Integration tests of `mychess_pgn::epd` (slice 4, part A of M1).
//!
//! Derived from `docs/specs/m1-4-epd-eco.md` (rules 1-19, decisions D39-D47)
//! and from the Scid's `pbook.cpp` (`Insert`, `ReadFile`, `WriteFile`,
//! `StripOpcode`, `FindSummary`). The reference tree ships no `.epd` file, so
//! there are no `compat_*` tests (see the note at the end of the file).

#![allow(clippy::assert_is_empty)]

use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Instant;

use mychess_core::{Color, Game, Move, Position, SanMode};
use mychess_pgn::epd::{
    EpdBook, EpdDiagnosticKind, EpdError, EpdIndex, EpdLoad, EpdOperation, EpdOptions, EpdRecord,
    InsertResult,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

const START4: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";
const E4_4: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -";
const D4_4: &str = "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq -";

// ---------------------------------------------------------------- helpers

fn read(bytes: impl AsRef<[u8]>) -> EpdLoad {
    EpdBook::read(bytes.as_ref(), EpdOptions::default()).expect("in-memory read cannot fail")
}

fn scid() -> EpdOptions {
    EpdOptions::scid()
}

fn read_scid(bytes: impl AsRef<[u8]>) -> EpdLoad {
    EpdBook::read(bytes.as_ref(), scid()).expect("in-memory read cannot fail")
}

fn op(opcode: &str, operands: &str) -> EpdOperation {
    EpdOperation::new(opcode, operands)
}

fn pairs(record: &EpdRecord) -> Vec<(&str, &str)> {
    record
        .operations()
        .iter()
        .map(|o| (o.opcode.as_str(), o.operands.as_str()))
        .collect()
}

fn after(moves: &str) -> Position {
    let mut pos = Position::new();
    for san in moves.split_whitespace() {
        let mv = pos.parse_san(san, SanMode::Strict).expect("legal").mv;
        pos = pos.after(mv).expect("legal");
    }
    pos
}

fn fen(text: &str) -> Position {
    Position::from_fen(text).expect("valid fen")
}

fn text_of(book: &EpdBook, options: EpdOptions) -> String {
    let mut out = Vec::new();
    book.write(&mut out, options).expect("write");
    String::from_utf8(out).expect("utf-8 output")
}

fn single_record_book(position: Position, ops: Vec<EpdOperation>) -> EpdBook {
    let mut book = EpdBook::new();
    book.insert(position, ops);
    book
}

fn kinds(load: &EpdLoad) -> Vec<(u64, &EpdDiagnosticKind)> {
    load.diagnostics.iter().map(|d| (d.line, &d.kind)).collect()
}

fn is_invalid_position(kind: &EpdDiagnosticKind) -> bool {
    matches!(kind, EpdDiagnosticKind::InvalidPosition { .. })
}

/// A reader that yields at most one byte per call.
struct OneByte<'a>(&'a [u8]);

impl Read for OneByte<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match (self.0.split_first(), buf.first_mut()) {
            (Some((&b, rest)), Some(slot)) => {
                *slot = b;
                self.0 = rest;
                Ok(1)
            }
            _ => Ok(0),
        }
    }
}

/// Yields its data, then fails.
struct FailsAfter {
    data: Vec<u8>,
    pos: usize,
}

impl Read for FailsAfter {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.data.len() {
            return Err(io::Error::other("disk on fire"));
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Fails once with `Interrupted` before every successful read.
struct Interrupting<'a> {
    data: &'a [u8],
    interrupt_next: bool,
}

impl Read for Interrupting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.interrupt_next {
            self.interrupt_next = false;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        self.interrupt_next = true;
        let n = buf.len().min(self.data.len()).min(7);
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

// ---------------------------------------------------------------- 1. reading

#[test]
fn options_default_to_standard_dialect_without_clocks() {
    let o = EpdOptions::default();
    assert!(!o.scid_escapes);
    assert!(!o.write_clocks);
    assert_eq!(o, EpdOptions::standard());
    assert_eq!(o.max_records, None, "no record limit by default");
    assert_eq!(EpdOptions::scid(), o.with_scid_escapes(true));
    assert!(EpdOptions::scid().scid_escapes);
}

#[test]
fn reads_typical_epd_line() {
    let load = read(format!("{START4} bm e4; id \"x\";\n"));

    assert!(load.diagnostics.is_empty(), "{:?}", load.diagnostics);
    assert_eq!(load.dropped_diagnostics, 0);
    assert_eq!(load.book.len(), 1);
    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("bm", "e4"), ("id", "\"x\"")]);
    assert!(
        !load.book.is_modified(),
        "a freshly read book is unmodified"
    );
}

#[test]
fn reads_the_spec_example_line() {
    let line = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - bm Bb5; id \"ruy\"; c0 \"a ; semicolon in a string\";\n";
    let load = read(line);

    assert!(load.diagnostics.is_empty(), "{:?}", load.diagnostics);
    let pos = after("e4 e5 Nf3 Nc6");
    let rec = load.book.get(&pos).expect("record");
    assert_eq!(
        pairs(rec),
        [
            ("bm", "Bb5"),
            ("id", "\"ruy\""),
            ("c0", "\"a ; semicolon in a string\"")
        ]
    );
}

#[test]
fn semicolon_inside_quotes_is_text_in_the_standard_dialect() {
    let load = read(format!("{START4} c0 \"a ; b\"; id z;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("c0", "\"a ; b\""), ("id", "z")]);
}

#[test]
fn backslash_is_literal_in_the_standard_dialect() {
    let load = read(format!("{START4} c0 a\\sb \\\\;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("c0", "a\\sb \\\\")]);
}

#[test]
fn scid_dialect_decodes_escapes() {
    // pbook.cpp:685-692: `\s` -> `;`, any other `\x` -> `x` (so `\\` -> `\`).
    let load = read_scid(format!("{START4} c0 a\\sb \\\\ \\q; id z;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("c0", "a;b \\ q"), ("id", "z")]);
    assert!(load.diagnostics.is_empty());
}

#[test]
fn scid_dialect_cuts_at_every_unescaped_semicolon_even_inside_quotes() {
    // pbook.cpp:680: `while (*s != ';' && *s != 0)` never looks at quotes, so the
    // implementer's choice agrees with the Scid.
    let load = read_scid(format!("{START4} c0 \"a;b\"; id z;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("c0", "\"a"), ("b\"", ""), ("id", "z")]);
    let nonstandard: Vec<_> = load
        .diagnostics
        .iter()
        .filter(|d| matches!(d.kind, EpdDiagnosticKind::NonStandardOpcode { .. }))
        .collect();
    assert_eq!(
        nonstandard.len(),
        1,
        "the piece `b\"` is not a standard opcode"
    );
}

#[test]
fn scid_dialect_does_not_track_quotes_so_no_unterminated_string() {
    let load = read_scid(format!("{START4} c0 \"abc;\n"));

    assert!(load.diagnostics.is_empty(), "{:?}", load.diagnostics);
    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("c0", "\"abc")]);
}

#[test]
fn scid_dialect_trailing_backslash_does_not_panic() {
    let load = read_scid(format!("{START4} c0 abc\\"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(
        rec.operation("c0").map(|o| o.operands.as_str()),
        Some("abc")
    );
}

#[test]
fn reads_six_field_fen_clocks_and_does_not_make_them_operations() {
    let load = read(format!("{START4} 5 10 bm e4;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(rec.position().halfmove_clock(), 5);
    assert_eq!(rec.position().fullmove_number(), 10);
    assert_eq!(pairs(rec), [("bm", "e4")]);
    assert!(load.diagnostics.is_empty());
}

#[test]
fn four_field_line_has_default_clocks() {
    let load = read(format!("{START4} bm e4;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(rec.position().halfmove_clock(), 0);
    assert_eq!(rec.position().fullmove_number(), 1);
}

#[test]
fn clocks_are_taken_only_when_both_are_numeric() {
    // D43: only "both numeric" counts as a six-field FEN.
    let load = read(format!("{START4} 5 x bm e4;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(rec.position().halfmove_clock(), 0);
    assert_eq!(rec.position().fullmove_number(), 1);
    assert_eq!(pairs(rec), [("5", "x bm e4")]);
    assert!(
        load.diagnostics.iter().any(
            |d| matches!(&d.kind, EpdDiagnosticKind::NonStandardOpcode { opcode } if opcode == "5")
        ),
        "{:?}",
        load.diagnostics
    );
}

#[test]
fn ignores_blank_lines_and_comment_lines_silently() {
    let text = format!("\n   \n# a comment\n% another\n{START4} id a;\n\n");
    let load = read(text);

    assert!(load.diagnostics.is_empty(), "{:?}", load.diagnostics);
    assert_eq!(load.book.len(), 1);
}

#[test]
fn invalid_fen_is_reported_with_its_line_and_reading_goes_on() {
    let text = format!("{START4} id a;\n\nthis is not a fen at all;\n{E4_4} id b;\n");
    let load = read(text);

    assert_eq!(load.book.len(), 2);
    assert_eq!(load.diagnostics.len(), 1, "{:?}", load.diagnostics);
    let d = &load.diagnostics[0];
    assert_eq!(d.line, 3);
    assert!(is_invalid_position(&d.kind), "{:?}", d.kind);
}

#[test]
fn illegal_position_is_invalid_position() {
    // Two white kings.
    let load = read("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBKR w - -\n");

    assert!(load.book.is_empty());
    assert_eq!(load.diagnostics.len(), 1);
    assert_eq!(load.diagnostics[0].line, 1);
    assert!(is_invalid_position(&load.diagnostics[0].kind));
}

#[rstest]
#[case::lf("\n")]
#[case::crlf("\r\n")]
#[case::cr("\r")]
fn line_numbers_count_every_terminator_style_once(#[case] sep: &str) {
    let text = format!("{START4} id a;{sep}{sep}garbage{sep}{E4_4} id b;{sep}");
    let load = read(text);

    assert_eq!(load.book.len(), 2);
    let k = kinds(&load);
    assert_eq!(k.len(), 1, "{k:?}");
    assert_eq!(k[0].0, 3, "bad line is the third (blank line counts)");
    assert!(is_invalid_position(k[0].1));
}

#[test]
fn mixed_line_terminators_in_one_file() {
    let text = format!("{START4} id a;\r\n{E4_4} id b;\r{D4_4} id c;\nbad\r\n");
    let load = read(text);

    assert_eq!(load.book.len(), 3);
    assert_eq!(load.diagnostics.len(), 1);
    assert_eq!(load.diagnostics[0].line, 4);
}

#[test]
fn crlf_split_across_reads_is_one_terminator() {
    let text = format!("{START4} id a;\r\nbad\r\n{E4_4} id b;\r\n");
    let load = EpdBook::read(OneByte(text.as_bytes()), EpdOptions::default()).expect("read");

    assert_eq!(load.book.len(), 2);
    assert_eq!(load.diagnostics.len(), 1);
    assert_eq!(load.diagnostics[0].line, 2);
}

#[test]
fn interrupted_reads_are_retried() {
    let text = format!("{START4} id a;\n{E4_4} id b;\n");
    let reader = Interrupting {
        data: text.as_bytes(),
        interrupt_next: true,
    };
    let load = EpdBook::read(reader, EpdOptions::default()).expect("read");

    assert_eq!(load.book.len(), 2);
}

#[test]
fn last_line_without_newline_is_processed() {
    // D44: the Scid loses it (`while (!EndOfFile())`, pbook.cpp:659); we do not.
    let load = read(format!("{START4} id a;\n{E4_4} id b;"));

    assert_eq!(load.book.len(), 2);
    let rec = load.book.get(&after("e4")).expect("last record");
    assert_eq!(pairs(rec), [("id", "b")]);
}

#[test]
fn empty_input_gives_empty_book() {
    let load = read("");

    assert!(load.book.is_empty());
    assert!(load.diagnostics.is_empty());
    assert_eq!(load.book.last_index(), None);
}

#[test]
fn utf8_bom_at_start_is_skipped() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(format!("{START4} id a;\n").as_bytes());
    let load = read(bytes);

    assert!(load.diagnostics.is_empty(), "{:?}", load.diagnostics);
    assert_eq!(load.book.len(), 1);
}

#[test]
fn utf8_text_is_preserved() {
    let load = read(format!(
        "{START4} c0 \"caf\u{e9} \u{4e2d}\u{6587} \u{1F600}\";\n"
    ));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(rec.comment(0), Some("caf\u{e9} \u{4e2d}\u{6587} \u{1F600}"));
}

#[test]
fn latin1_and_windows_1252_text_is_decoded() {
    let mut bytes = format!("{START4} c0 \"caf").into_bytes();
    bytes.extend_from_slice(b"\xE9 "); // e acute in Latin-1 / windows-1252
    bytes.extend_from_slice(b"\x92 "); // right single quote in windows-1252
    bytes.extend_from_slice(b"\x80"); // euro sign in windows-1252
    bytes.extend_from_slice(b"\";\n");
    let load = read(bytes);

    assert!(load.diagnostics.is_empty(), "{:?}", load.diagnostics);
    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(rec.comment(0), Some("caf\u{e9} \u{2019} \u{20ac}"));
}

#[test]
fn line_of_exactly_one_mebibyte_is_accepted_and_one_more_byte_is_not() {
    let prefix = format!("{START4} c0 \"");
    let suffix = "\";";
    let fill = (1 << 20) - prefix.len() - suffix.len();
    let ok_line = format!("{prefix}{}{suffix}", "x".repeat(fill));
    assert_eq!(ok_line.len(), 1 << 20);
    let long_line = format!("{prefix}{}{suffix}", "x".repeat(fill + 1));

    let text = format!("{ok_line}\n{long_line}\n{E4_4} id b;\n");
    let load = read(text);

    assert_eq!(load.book.len(), 2, "the exact-limit line and the last one");
    assert_eq!(kinds(&load), vec![(2, &EpdDiagnosticKind::LineTooLong)]);
    assert!(
        load.book.contains(&after("e4")),
        "reading goes on after a long line"
    );
}

#[test]
fn long_line_without_terminator_at_end_of_file_is_reported() {
    let long = format!("{START4} c0 \"{}\";", "y".repeat(2 << 20));
    let load = read(format!("{START4} id a;\n{long}"));

    assert_eq!(load.book.len(), 1);
    assert_eq!(kinds(&load), vec![(2, &EpdDiagnosticKind::LineTooLong)]);
}

#[test]
fn unterminated_quote_runs_to_end_of_line_and_is_reported() {
    let text = format!("{START4} id a; c0 \"never closed; id x;\n{E4_4} id b;\n");
    let load = read(text);

    assert_eq!(
        kinds(&load),
        vec![(1, &EpdDiagnosticKind::UnterminatedString)]
    );
    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("id", "a"), ("c0", "\"never closed; id x;")]);
    assert_eq!(load.book.len(), 2, "the next line is not swallowed");
}

#[test]
fn non_standard_opcode_is_kept_and_reported() {
    let load = read(format!("{START4} 1x foo; ok bar;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("1x", "foo"), ("ok", "bar")]);
    assert_eq!(
        kinds(&load),
        vec![(
            1,
            &EpdDiagnosticKind::NonStandardOpcode {
                opcode: "1x".to_owned()
            }
        )]
    );
}

#[test]
fn operation_without_operands_and_empty_operations() {
    let load = read(format!("{START4} ;; noop;  ;  bm  e4 ;   ;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("noop", ""), ("bm", "e4")]);
    assert!(load.diagnostics.is_empty());
}

#[test]
fn line_with_only_the_position_has_no_operations() {
    let load = read(format!("{START4}\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert!(rec.operations().is_empty());
}

#[test]
fn duplicate_opcodes_in_a_line_are_all_kept_in_order() {
    let load = read(format!("{START4} id a; bm e4; id b;\n"));

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(pairs(rec), [("id", "a"), ("bm", "e4"), ("id", "b")]);
    assert_eq!(rec.operation("id").map(|o| o.operands.as_str()), Some("a"));
    let all: Vec<_> = rec
        .operations_named("id")
        .map(|o| o.operands.as_str())
        .collect();
    assert_eq!(all, ["a", "b"]);
}

#[test]
fn repeated_position_last_wins_keeps_first_place_and_last_clocks() {
    let text = format!(
        "{START4} id first;\n{E4_4} id middle;\n{START4} 5 9 id last;\n{START4} id again;\n"
    );
    let load = read(text);

    assert_eq!(load.book.len(), 2);
    let order: Vec<_> = load
        .book
        .iter()
        .map(|r| r.operation("id").map(|o| o.operands.clone()))
        .collect();
    assert_eq!(order, [Some("again".to_owned()), Some("middle".to_owned())]);
    assert_eq!(
        kinds(&load),
        vec![
            (3, &EpdDiagnosticKind::DuplicatePosition { first_line: 1 }),
            (4, &EpdDiagnosticKind::DuplicatePosition { first_line: 1 }),
        ]
    );
    let first = load.book.iter_indexed().next().map(|(i, _)| i);
    assert_eq!(load.book.index_of(&Position::new()), first);
}

#[test]
fn repeated_position_takes_clocks_of_the_last_line() {
    let text = format!("{START4} 3 4 id a;\n{START4} 5 9 id b;\n");
    let load = read(text);

    let rec = load.book.get(&Position::new()).expect("record");
    assert_eq!(rec.position().halfmove_clock(), 5);
    assert_eq!(rec.position().fullmove_number(), 9);
}

#[test]
fn invalid_lines_do_not_count_as_duplicates() {
    let load = read(format!("{START4} id a;\nbad line\n{START4} id b;\n"));

    assert_eq!(load.book.len(), 1);
    assert_eq!(
        kinds(&load).iter().map(|(l, _)| *l).collect::<Vec<_>>(),
        [2, 3]
    );
}

#[test]
fn en_passant_that_is_illegal_gives_the_same_position_as_none() {
    // D40 (observation, a conscious divergence from the Scid): the white pawn on
    // e5 is pinned along the fifth rank, so `d6` is only pseudo-legal. The
    // Scid keeps the two positions apart; we identify them (FIDE rule).
    let with_ep = "8/8/8/K2pP2r/8/8/8/7k w - d6";
    let without = "8/8/8/K2pP2r/8/8/8/7k w - -";
    let load = read(format!("{with_ep} id a;\n{without} id b;\n"));

    assert_eq!(load.book.len(), 1);
    assert_eq!(load.diagnostics.len(), 1);
    assert!(matches!(
        load.diagnostics[0].kind,
        EpdDiagnosticKind::DuplicatePosition { first_line: 1 }
    ));
}

#[test]
fn en_passant_that_is_legal_distinguishes_positions() {
    let with_ep = "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6";
    let without = "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq -";
    let load = read(format!("{with_ep} id a;\n{without} id b;\n"));

    assert_eq!(load.book.len(), 2);
}

#[test]
fn read_file_reads_plain_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("book.epd");
    std::fs::write(&path, format!("{START4} id a;\n")).expect("write");

    let load = EpdBook::read_file(&path, EpdOptions::default()).expect("read");

    assert_eq!(load.book.len(), 1);
    assert!(!load.book.is_modified());
}

#[test]
fn read_file_decompresses_gz() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("book.epd.gz");
    let mut enc = flate2::write::GzEncoder::new(
        std::fs::File::create(&path).expect("create"),
        flate2::Compression::default(),
    );
    enc.write_all(format!("{START4} id a;\r\n{E4_4} id b;\r\n").as_bytes())
        .expect("compress");
    enc.finish().expect("finish");

    let load = EpdBook::read_file(&path, EpdOptions::default()).expect("read gz");

    assert_eq!(load.book.len(), 2);
    assert!(load.diagnostics.is_empty());
}

#[test]
fn gzip_is_detected_by_magic_bytes_not_by_file_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gz_named_plain = dir.path().join("book.epd");
    let plain_named_gz = dir.path().join("plain.epd.gz");
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(format!("{START4} id a;\n").as_bytes())
        .expect("compress");
    std::fs::write(&gz_named_plain, enc.finish().expect("finish")).expect("write");
    std::fs::write(&plain_named_gz, format!("{START4} id a;\n")).expect("write");

    for path in [&gz_named_plain, &plain_named_gz] {
        let load = EpdBook::read_file(path, EpdOptions::default()).expect("read");
        assert_eq!(load.book.len(), 1, "{}", path.display());
    }
}

#[test]
fn gzip_stream_is_inflated_by_read_from_memory_too() {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(format!("{START4} id a;\n").as_bytes())
        .expect("compress");
    let bytes = enc.finish().expect("finish");

    let load = EpdBook::read(bytes.as_slice(), EpdOptions::default()).expect("read");

    assert_eq!(load.book.len(), 1);
}

#[test]
fn concatenated_gzip_members_are_all_read() {
    let member = |text: &str| {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(text.as_bytes()).expect("compress");
        enc.finish().expect("finish")
    };
    let mut bytes = member(&format!("{START4} id a;\n"));
    bytes.extend(member(&format!("{E4_4} id b;\n")));

    let load = EpdBook::read(bytes.as_slice(), EpdOptions::default()).expect("read");

    assert_eq!(load.book.len(), 2);
}

#[test]
fn decompression_bomb_is_stopped_by_the_decompressed_size_limit() {
    let body = format!("{START4} id a;\n").repeat(50_000);
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(body.as_bytes()).expect("compress");
    let bytes = enc.finish().expect("finish");
    let tight = EpdOptions::standard().with_max_decompressed_bytes(Some(1000));

    let result = EpdBook::read(bytes.as_slice(), tight);

    assert!(
        matches!(result, Err(EpdError::LimitExceeded { max: 1000, .. })),
        "{result:?}"
    );
    let unlimited = EpdOptions::standard().with_max_decompressed_bytes(None);
    assert_eq!(
        EpdBook::read(bytes.as_slice(), unlimited)
            .expect("read")
            .book
            .len(),
        1
    );
}

#[test]
fn plain_input_is_not_subject_to_the_decompressed_size_limit() {
    let text = format!("{START4} id a;\n").repeat(100);
    let tight = EpdOptions::standard().with_max_decompressed_bytes(Some(10));

    let load = EpdBook::read(text.as_bytes(), tight).expect("plain input is not limited");

    assert_eq!(load.book.len(), 1);
}

#[test]
fn max_records_limits_distinct_positions_only() {
    let text = format!("{START4} id a;\n{E4_4} id b;\n{START4} id again;\n");
    let opts = EpdOptions::standard().with_max_records(Some(2));

    let load = EpdBook::read(text.as_bytes(), opts).expect("two distinct positions fit");
    assert_eq!(load.book.len(), 2);

    let three = format!("{text}{D4_4} id c;\n");
    let result = EpdBook::read(three.as_bytes(), opts);
    assert!(
        matches!(result, Err(EpdError::LimitExceeded { max: 2, .. })),
        "{result:?}"
    );
    let zero = EpdOptions::standard().with_max_records(Some(0));
    assert!(matches!(
        EpdBook::read(text.as_bytes(), zero),
        Err(EpdError::LimitExceeded { max: 0, .. })
    ));
    assert!(
        EpdBook::read("".as_bytes(), zero).is_ok(),
        "an empty input holds no record"
    );
}

#[test]
fn write_file_refuses_gz_paths_without_touching_anything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.epd.gz");
    let book = single_record_book(Position::new(), vec![op("id", "a")]);

    let result = book.write_file(&path, EpdOptions::default());

    assert!(
        matches!(result, Err(EpdError::Unsupported { .. })),
        "{result:?}"
    );
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 0);
}

#[test]
fn read_file_of_a_truncated_gz_is_an_io_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("book.epd.gz");
    let mut data = Vec::new();
    {
        let mut enc = flate2::write::GzEncoder::new(&mut data, flate2::Compression::none());
        let body = format!("{START4} id a;\n").repeat(2000);
        enc.write_all(body.as_bytes()).expect("compress");
        enc.finish().expect("finish");
    }
    data.truncate(data.len() - 40);
    std::fs::write(&path, data).expect("write");

    let result = EpdBook::read_file(&path, EpdOptions::default());

    match result {
        Err(EpdError::Io { path: Some(p), .. }) => assert_eq!(p, path),
        Ok(_) => panic!("a truncated gzip stream must not be accepted silently"),
        Err(other) => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn read_file_of_missing_file_is_io_error_with_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("missing.epd");

    let result = EpdBook::read_file(&path, EpdOptions::default());

    match result {
        Err(EpdError::Io {
            path: Some(p),
            source,
        }) => {
            assert_eq!(p, path);
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn io_error_in_the_middle_of_the_stream_is_an_error_not_a_partial_book() {
    let reader = FailsAfter {
        data: format!("{START4} id a;\n{E4_4} id b;\n").into_bytes(),
        pos: 0,
    };

    let result = EpdBook::read(reader, EpdOptions::default());

    match result {
        Err(EpdError::Io { path: None, source }) => assert_eq!(source.to_string(), "disk on fire"),
        Ok(_) => panic!("an I/O error must be reported"),
        Err(other) => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn chunked_reading_gives_the_same_book_as_whole_reading() {
    let text = format!("# c\r\n{START4} c0 \"a ; b\"; id \u{e9};\r{E4_4}\nbad\n{START4} id z;");

    let whole = read(&text);
    let chunked = EpdBook::read(OneByte(text.as_bytes()), EpdOptions::default()).expect("read");

    assert_eq!(whole.book, chunked.book);
    assert_eq!(whole.diagnostics, chunked.diagnostics);
}

// ---------------------------------------------------------------- 2. the book

fn three_record_book() -> (EpdBook, Position, Position, Position) {
    let (a, b, c) = (Position::new(), after("e4"), after("d4"));
    let mut book = EpdBook::new();
    assert_eq!(
        book.insert(a.clone(), vec![op("id", "a")]),
        InsertResult::Added
    );
    assert_eq!(
        book.insert(b.clone(), vec![op("id", "b")]),
        InsertResult::Added
    );
    assert_eq!(
        book.insert(c.clone(), vec![op("id", "c")]),
        InsertResult::Added
    );
    (book, a, b, c)
}

fn ids(book: &EpdBook) -> Vec<String> {
    book.iter()
        .map(|r| r.id().unwrap_or("").to_owned())
        .collect()
}

#[test]
fn new_book_is_empty_and_unmodified_and_writable() {
    let book = EpdBook::new();

    assert!(book.is_empty());
    assert_eq!(book.len(), 0);
    assert!(!book.is_modified());
    assert!(!book.is_read_only());
    assert_eq!(book.last_index(), None);
}

#[test]
fn insert_keeps_insertion_order_and_marks_modified() {
    let (book, a, b, c) = three_record_book();

    assert_eq!(ids(&book), ["a", "b", "c"]);
    assert_eq!(book.len(), 3);
    assert!(book.is_modified());
    let idx: Vec<EpdIndex> = [&a, &b, &c]
        .iter()
        .map(|p| book.index_of(p).expect("indexed"))
        .collect();
    assert!(
        idx.windows(2).all(|w| w[0] < w[1]),
        "handles grow in file order"
    );
    let walked: Vec<EpdIndex> = book.iter_indexed().map(|(i, _)| i).collect();
    assert_eq!(walked, idx);
}

#[test]
fn insert_of_existing_position_replaces_data_and_keeps_place() {
    let (mut book, _a, b, _c) = three_record_book();
    let before = book.index_of(&b);

    let result = book.insert(b.clone(), vec![op("id", "B2"), op("bm", "e5")]);

    assert_eq!(result, InsertResult::Replaced);
    assert_eq!(book.len(), 3);
    assert_eq!(book.index_of(&b), before);
    assert_eq!(ids(&book), ["a", "B2", "c"]);
    assert_eq!(book.get(&b).map(|r| r.operations().len()), Some(2));
}

#[test]
fn insert_ignores_clocks_in_the_key() {
    let mut book = EpdBook::new();
    book.insert(
        fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
        vec![],
    );

    let again = fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 7 30");

    assert!(book.contains(&again));
    assert_eq!(book.insert(again, vec![]), InsertResult::Replaced);
    assert_eq!(book.len(), 1);
}

#[test]
fn get_and_contains_for_absent_position() {
    let (book, ..) = three_record_book();
    let other = after("c4");

    assert!(!book.contains(&other));
    assert!(book.get(&other).is_none());
    assert!(book.index_of(&other).is_none());
}

#[test]
fn append_position_adds_once_and_keeps_existing_operations() {
    let mut book = EpdBook::new();
    let p = after("e4");

    assert!(book.append_position(&p));
    assert!(book.get(&p).expect("added").operations().is_empty());
    book.insert(p.clone(), vec![op("id", "keep")]);
    assert!(!book.append_position(&p));

    assert_eq!(book.len(), 1);
    assert_eq!(book.get(&p).and_then(EpdRecord::id), Some("keep"));
}

#[test]
fn remove_returns_the_record_and_keeps_other_indices_stable() {
    let (mut book, a, b, c) = three_record_book();
    let (ia, ib, ic) = (
        book.index_of(&a).expect("a"),
        book.index_of(&b).expect("b"),
        book.index_of(&c).expect("c"),
    );
    book.clear_modified();

    let removed = book.remove(&b);

    assert_eq!(
        removed.and_then(|r| r.id().map(str::to_owned)),
        Some("b".to_owned())
    );
    assert!(book.is_modified());
    assert_eq!(book.len(), 2);
    assert!(!book.contains(&b));
    assert_eq!(book.index_of(&a), Some(ia));
    assert_eq!(book.index_of(&c), Some(ic), "c keeps its handle");
    assert!(book.get_by_index(ib).is_none(), "the hole is empty");
    assert_eq!(ids(&book), ["a", "c"]);
    let indexed: Vec<EpdIndex> = book.iter_indexed().map(|(i, _)| i).collect();
    assert_eq!(indexed, [ia, ic]);
}

#[test]
fn remove_of_absent_position_returns_none() {
    let (mut book, ..) = three_record_book();

    assert!(book.remove(&after("c4")).is_none());
    assert_eq!(book.len(), 3);
}

#[test]
fn navigation_is_cyclic_and_skips_removed_records() {
    let (mut book, a, b, c) = three_record_book();
    let last = book.last_index().expect("last");
    assert_eq!(
        Some(last),
        book.index_of(&c),
        "the cursor starts at the last record"
    );
    let first = book.next(last).expect("wraps to first");
    assert_eq!(first, book.index_of(&a).expect("a"));
    book.remove(&b);

    let ia = book.index_of(&a).expect("a");
    let ic = book.index_of(&c).expect("c");
    assert_eq!(book.next(ia), Some(ic), "next skips the hole");
    assert_eq!(book.prev(ic), Some(ia), "prev skips the hole");
    assert_eq!(book.next(ic), Some(ia), "next wraps");
    assert_eq!(book.prev(ia), Some(ic), "prev wraps");
}

#[test]
fn navigation_from_a_removed_index_still_finds_a_live_record() {
    let (mut book, a, b, c) = three_record_book();
    let ib = book.index_of(&b).expect("b");
    book.remove(&b);

    assert_eq!(book.next(ib), book.index_of(&c));
    assert_eq!(book.prev(ib), book.index_of(&a));
}

#[test]
fn navigation_with_the_trailing_records_removed() {
    let (mut book, a, b, c) = three_record_book();
    let ic = book.index_of(&c).expect("c");
    book.remove(&c);
    book.remove(&b);

    let ia = book.index_of(&a).expect("a");
    assert_eq!(book.last_index(), Some(ia), "last live record");
    assert_eq!(book.next(ia), Some(ia));
    assert_eq!(book.prev(ia), Some(ia));
    assert_eq!(book.next(ic), Some(ia));
    assert_eq!(book.prev(ic), Some(ia));
}

#[test]
fn navigation_in_an_emptied_book_is_none() {
    let (mut book, a, b, c) = three_record_book();
    let ia = book.index_of(&a).expect("a");
    for p in [&a, &b, &c] {
        book.remove(p);
    }

    assert!(book.is_empty());
    assert_eq!(book.next(ia), None);
    assert_eq!(book.prev(ia), None);
    assert_eq!(book.last_index(), None);
}

#[test]
fn cursor_after_open_starts_at_last_so_first_next_is_the_first_record() {
    let load = read(format!(
        "{START4} id a;\n{E4_4} id b;\n{START4} id again;\n{D4_4} id c;\n"
    ));
    let book = load.book;

    let cursor = book.last_index().expect("cursor");
    let first = book.next(cursor).expect("first");

    assert_eq!(
        Some(first),
        book.iter_indexed().next().map(|(i, _)| i),
        "the first `next` reaches the first record"
    );
    assert_eq!(
        book.get_by_index(first).and_then(EpdRecord::id),
        Some("again")
    );
    assert_eq!(Some(cursor), book.index_of(&after("d4")));
}

#[test]
fn strip_opcode_removes_every_occurrence_and_counts_changed_records() {
    let (mut book, a, b, c) = three_record_book();
    book.get_mut(&a).expect("a").set_operation("bm", "e4");
    book.insert(
        b.clone(),
        vec![op("id", "x"), op("bm", "e5"), op("id", "y"), op("noop", "")],
    );
    book.insert(c.clone(), vec![op("bm", "d5")]);
    book.clear_modified();

    // a: [id, bm]; b: [id, bm, id, noop]; c: [bm]
    assert_eq!(book.strip_opcode("id"), 2, "records a and b changed, not c");
    assert_eq!(
        pairs(book.get(&b).expect("b")),
        [("bm", "e5"), ("noop", "")]
    );
    assert_eq!(pairs(book.get(&a).expect("a")), [("bm", "e4")]);
    assert!(book.is_modified());
    assert_eq!(book.strip_opcode("id"), 0, "nothing left to strip");
}

#[test]
fn strip_opcode_counts_records_not_occurrences() {
    let mut book = EpdBook::new();
    book.insert(
        Position::new(),
        vec![op("id", "1"), op("id", "2"), op("id", "3")],
    );

    assert_eq!(book.strip_opcode("id"), 1);
    assert!(
        book.get(&Position::new())
            .expect("record")
            .operations()
            .is_empty()
    );
}

#[test]
fn strip_opcode_removes_noop_without_operands() {
    // The Scid searches "noop " (with a space) and never finds `noop;`
    // (pbook.cpp:485-503); D47 fixes it.
    let mut book = EpdBook::new();
    book.insert(Position::new(), vec![op("noop", ""), op("id", "a")]);

    assert_eq!(book.strip_opcode("noop"), 1);

    assert_eq!(
        pairs(book.get(&Position::new()).expect("record")),
        [("id", "a")]
    );
}

#[test]
fn strip_opcode_matches_whole_opcodes_only() {
    let mut book = EpdBook::new();
    book.insert(
        Position::new(),
        vec![op("bm", "e4"), op("bma", "x"), op("b", "y"), op("noop", "")],
    );

    assert_eq!(book.strip_opcode("b"), 1, "only `b`");
    assert_eq!(book.strip_opcode("no"), 0, "a prefix is not an opcode");
    assert_eq!(book.strip_opcode(""), 0);
    assert_eq!(
        book.get(&Position::new()).map(|r| r.operations().len()),
        Some(3)
    );
}

#[test]
fn strip_opcode_that_changes_nothing_leaves_the_book_unmodified() {
    let mut book = read(format!("{START4} id a;\n")).book;

    assert_eq!(book.strip_opcode("zz"), 0);

    assert!(!book.is_modified());
}

#[test]
fn sort_operations_orders_by_bytes_of_opcode_and_operands() {
    let mut book = EpdBook::new();
    book.insert(
        Position::new(),
        vec![
            op("id", "\"x\""),
            op("ce", "5"),
            op("bma", ""),
            op("bm", "e4"),
            op("bm", ""),
        ],
    );
    book.insert(after("e4"), vec![op("a", ""), op("b", "")]);
    book.clear_modified();

    let changed = book.sort_operations();

    assert_eq!(changed, 1, "only the first record was out of order");
    assert_eq!(
        pairs(book.get(&Position::new()).expect("record")),
        [
            ("bm", ""),
            ("bm", "e4"),
            ("bma", ""),
            ("ce", "5"),
            ("id", "\"x\"")
        ]
    );
    assert!(book.is_modified());
    assert_eq!(book.sort_operations(), 0, "idempotent");
}

#[test]
fn sort_operations_is_stable_and_leaves_sorted_book_unmodified() {
    let mut book = EpdBook::new();
    book.insert(Position::new(), vec![op("id", "b"), op("id", "a")]);
    book.clear_modified();
    assert_eq!(book.sort_operations(), 1);
    assert_eq!(
        pairs(book.get(&Position::new()).unwrap()),
        [("id", "a"), ("id", "b")]
    );

    book.clear_modified();
    assert_eq!(book.sort_operations(), 0);
    assert!(!book.is_modified());
}

#[test]
fn sort_operations_keeps_record_order() {
    let (mut book, ..) = three_record_book();
    book.get_mut(&after("e4"))
        .expect("b")
        .set_operation("a", "1");

    book.sort_operations();

    assert_eq!(ids(&book), ["a", "b", "c"]);
}

#[test]
fn get_mut_marks_the_book_modified_only_when_found() {
    let mut book = read(format!("{START4} id a;\n")).book;
    assert!(!book.is_modified());

    assert!(book.get_mut(&after("e4")).is_none());
    assert!(!book.is_modified(), "a miss changes nothing");

    book.get_mut(&Position::new())
        .expect("record")
        .set_operation("id", "z");
    assert!(book.is_modified());
    assert_eq!(
        book.get(&Position::new()).and_then(EpdRecord::id),
        Some("z")
    );
    book.clear_modified();
    assert!(!book.is_modified());
}

#[test]
fn read_only_flag_is_plain_state() {
    let mut book = EpdBook::new();
    book.set_read_only(true);
    assert!(book.is_read_only());
    book.insert(Position::new(), vec![]);
    assert_eq!(
        book.len(),
        1,
        "the flag is policy of the caller, not enforced"
    );
    book.set_read_only(false);
    assert!(!book.is_read_only());
}

#[test]
fn equality_compares_records_in_order_and_ignores_flags_and_holes() {
    let (mut with_hole, a, b, c) = three_record_book();
    with_hole.remove(&b);
    let mut fresh = EpdBook::new();
    fresh.insert(a.clone(), vec![op("id", "a")]);
    fresh.insert(c.clone(), vec![op("id", "c")]);
    fresh.set_read_only(true);
    fresh.clear_modified();

    assert_eq!(with_hole, fresh);

    let mut reversed = EpdBook::new();
    reversed.insert(c, vec![op("id", "c")]);
    reversed.insert(a, vec![op("id", "a")]);
    assert_ne!(with_hole, reversed, "order matters");
}

#[test]
fn equality_sees_operation_differences() {
    let one = single_record_book(Position::new(), vec![op("id", "a")]);
    let two = single_record_book(Position::new(), vec![op("id", "b")]);
    let three = single_record_book(after("e4"), vec![op("id", "a")]);

    assert_ne!(one, two);
    assert_ne!(one, three);
    assert_eq!(one, one.clone());
}

fn game_of(moves: &str) -> Game {
    let mut game = Game::new();
    let mut parent = game.root();
    for san in moves.split_whitespace() {
        let mv = game
            .position_at(parent)
            .expect("pos")
            .parse_san(san, SanMode::Strict)
            .expect("legal")
            .mv;
        parent = game.add_move(parent, mv).expect("add");
    }
    game
}

#[test]
fn deepest_match_is_the_greatest_mainline_ply_in_the_book() {
    let game = game_of("e4 e5 Nf3 Nc6");
    let mut book = EpdBook::new();
    book.insert(after("e4"), vec![]);
    book.insert(after("e4 e5 Nf3"), vec![]);

    assert_eq!(book.deepest_match(&game), Some(3));
    book.remove(&after("e4 e5 Nf3"));
    assert_eq!(book.deepest_match(&game), Some(1));
    book.remove(&after("e4"));
    assert_eq!(book.deepest_match(&game), None);
}

#[test]
fn deepest_match_distinguishes_ply_zero_from_no_match() {
    let game = game_of("e4");
    let mut book = EpdBook::new();
    assert_eq!(book.deepest_match(&game), None, "empty book");

    book.insert(Position::new(), vec![]);

    assert_eq!(book.deepest_match(&game), Some(0));
}

#[test]
fn deepest_match_ignores_variations() {
    let mut game = game_of("e4 e5");
    let root = game.root();
    let d4 = game
        .position_at(root)
        .expect("pos")
        .parse_san("d4", SanMode::Strict)
        .expect("legal")
        .mv;
    let var = game.add_move(root, d4).expect("variation");
    let d5 = game
        .position_at(var)
        .expect("pos")
        .parse_san("d5", SanMode::Strict)
        .expect("legal")
        .mv;
    game.add_move(var, d5).expect("variation move");
    let book = single_record_book(after("d4 d5"), vec![]);

    assert_eq!(book.deepest_match(&game), None);
}

#[test]
fn deepest_match_finds_transpositions_and_the_return_to_the_start() {
    let game = game_of("Nf3 Nf6 Ng1 Ng8");
    let book = single_record_book(Position::new(), vec![]);

    assert_eq!(book.deepest_match(&game), Some(4));
}

#[test]
fn deepest_match_uses_the_start_position_of_a_fen_game() {
    let start = fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1");
    let game = Game::from_position(start.clone());
    let book = single_record_book(start, vec![]);

    assert_eq!(book.deepest_match(&game), Some(0));
}

#[test]
fn moves_in_book_lists_legal_moves_whose_result_is_in_the_book() {
    let mut book = EpdBook::new();
    book.insert(after("e4"), vec![]);
    book.insert(after("Nf3"), vec![]);
    book.insert(after("e4 e5"), vec![]); // two plies away: not a reply of the start

    let start = Position::new();
    let moves: Vec<Move> = book.moves_in_book(&start);

    let mut sans: Vec<String> = moves.iter().map(|&m| start.san(m).expect("san")).collect();
    sans.sort();
    assert_eq!(sans, ["Nf3", "e4"]);
}

#[test]
fn moves_in_book_is_empty_when_nothing_matches() {
    let book = single_record_book(after("e4 e5"), vec![]);

    assert!(book.moves_in_book(&Position::new()).is_empty());
    assert!(EpdBook::new().moves_in_book(&Position::new()).is_empty());
}

#[rstest]
#[case::white_positive(Position::new(), "ce 25", Some("+0.25"))]
#[case::white_negative(Position::new(), "ce -7", Some("-0.07"))]
#[case::white_zero(Position::new(), "ce 0", Some("+0.00"))]
#[case::white_pawn(Position::new(), "ce 100", Some("+1.00"))]
#[case::white_plus_sign(Position::new(), "ce +150", Some("+1.50"))]
#[case::white_large(Position::new(), "ce 12345", Some("+123.45"))]
fn summary_ce_for_white_to_move(
    #[case] pos: Position,
    #[case] operation: &str,
    #[case] expected: Option<&str>,
) {
    let (opcode, operands) = operation.split_once(' ').expect("op");
    let book = single_record_book(pos.clone(), vec![op(opcode, operands)]);

    assert_eq!(book.summary(&pos).as_deref(), expected);
}

#[rstest]
#[case("ce 25", "-0.25")]
#[case("ce -150", "+1.50")]
#[case("ce 0", "+0.00")]
fn summary_ce_is_seen_from_white_when_black_is_to_move(
    #[case] operation: &str,
    #[case] expected: &str,
) {
    let pos = after("e4");
    assert_eq!(pos.turn(), Color::Black);
    let (opcode, operands) = operation.split_once(' ').expect("op");
    let book = single_record_book(pos.clone(), vec![op(opcode, operands)]);

    assert_eq!(book.summary(&pos).as_deref(), Some(expected));
}

#[test]
fn summary_prefers_ce_over_everything_else() {
    let book = single_record_book(
        Position::new(),
        vec![op("eco", "\"C20\""), op("ce", "10"), op("bm", "e4")],
    );

    assert_eq!(book.summary(&Position::new()).as_deref(), Some("+0.10"));
}

#[test]
fn summary_falls_back_in_the_order_eco_nic_pv_pm_bm_id() {
    let all = [
        op("id", "\"i\""),
        op("bm", "bm1"),
        op("pm", "pm1"),
        op("pv", "pv1 pv2"),
        op("nic", "nic1"),
        op("eco", "eco1"),
    ];
    let expected = ["eco1", "nic1", "pv1 pv2", "pm1", "bm1", "\"i\""];
    let p = Position::new();

    for (n, want) in expected.iter().enumerate() {
        // Keep only the opcodes from the n-th priority downward, in a
        // scrambled file order.
        let keep: Vec<EpdOperation> = all
            .iter()
            .filter(|o| {
                let prio = ["eco", "nic", "pv", "pm", "bm", "id"]
                    .iter()
                    .position(|c| *c == o.opcode)
                    .expect("known");
                prio >= n
            })
            .cloned()
            .collect();
        let book = single_record_book(p.clone(), keep);

        assert_eq!(
            book.summary(&p).as_deref(),
            Some(*want),
            "priority level {n}"
        );
    }
}

#[test]
fn summary_is_none_without_record_or_useful_operations() {
    let book = single_record_book(
        Position::new(),
        vec![op("c0", "\"comment\""), op("noop", "")],
    );

    assert_eq!(book.summary(&Position::new()), None);
    assert_eq!(book.summary(&after("e4")), None, "position not in the book");
}

#[test]
fn summary_ignores_an_unparsable_ce_and_falls_back() {
    // Scid: strGetInteger("abc") is 0, giving "+0.00"; ours has no number to
    // show. We only require that nothing panics and the result is stable.
    let book = single_record_book(Position::new(), vec![op("ce", "abc"), op("id", "z")]);

    let s = book.summary(&Position::new());
    assert!(s.is_some(), "{s:?}");
}

// ---------------------------------------------------------------- 3. typed helpers

fn rec(pos: Position, ops: &[(&str, &str)]) -> EpdRecord {
    EpdRecord::with_operations(pos, ops.iter().map(|(o, a)| op(o, a)).collect())
}

#[test]
fn bm_and_am_resolve_san_in_the_record_position() {
    let r = rec(Position::new(), &[("bm", "e4 Nf3"), ("am", "f3")]);
    let start = Position::new();
    let e4 = start.parse_san("e4", SanMode::Strict).expect("e4").mv;
    let nf3 = start.parse_san("Nf3", SanMode::Strict).expect("Nf3").mv;
    let f3 = start.parse_san("f3", SanMode::Strict).expect("f3").mv;

    assert_eq!(r.bm().expect("bm"), vec![e4, nf3]);
    assert_eq!(r.am().expect("am"), vec![f3]);
}

#[test]
fn bm_uses_the_side_to_move_of_the_record() {
    let r = rec(after("e4"), &[("bm", "e5")]);

    assert_eq!(r.bm().map(|m| m.len()), Ok(1));
    assert!(
        rec(Position::new(), &[("bm", "e5")]).bm().is_err(),
        "e5 is illegal for White"
    );
}

#[test]
fn bm_with_an_illegal_move_is_an_error() {
    let r = rec(Position::new(), &[("bm", "e4 Qh5")]);

    assert!(r.bm().is_err());
}

#[test]
fn bm_am_pv_are_empty_without_the_operation() {
    let r = rec(Position::new(), &[("id", "x")]);

    assert_eq!(r.bm(), Ok(Vec::new()));
    assert_eq!(r.am(), Ok(Vec::new()));
    assert_eq!(r.pv(), Ok(Vec::new()));
}

#[test]
fn pv_plays_the_moves_in_sequence() {
    let ok = rec(Position::new(), &[("pv", "e4 e5 Nf3")]);
    let bad = rec(Position::new(), &[("pv", "e4 e4")]);

    assert_eq!(ok.pv().map(|m| m.len()), Ok(3));
    assert!(bad.pv().is_err(), "the second e4 is illegal for Black");
}

#[test]
fn bm_accepts_san_with_check_suffix_and_annotations() {
    let pos = after("e4 e5 Bc4 Nc6 Qh5 Nf6");
    let r = rec(pos, &[("bm", "Qxf7#")]);

    assert_eq!(r.bm().map(|m| m.len()), Ok(1));
}

#[rstest]
#[case("ce", "25", Some(25))]
#[case("ce", "-25", Some(-25))]
#[case("ce", "+30", Some(30))]
#[case("ce", "  42 ", Some(42))]
#[case("ce", "abc", None)]
#[case("ce", "", None)]
#[case("ce", "99999999999999999999", None)]
fn centipawns_parses_integers(
    #[case] code: &str,
    #[case] operands: &str,
    #[case] want: Option<i32>,
) {
    let r = rec(Position::new(), &[(code, operands)]);

    assert_eq!(r.centipawns(), want);
}

#[test]
fn centipawns_without_ce_is_none() {
    assert_eq!(rec(Position::new(), &[("dm", "3")]).centipawns(), None);
}

#[test]
fn dm_and_analysis_counters() {
    let r = rec(
        Position::new(),
        &[("dm", "3"), ("acd", "12"), ("acn", "1000000"), ("acs", "7")],
    );

    assert_eq!(r.dm(), Some(3));
    assert_eq!(r.acd(), Some(12));
    assert_eq!(r.acn(), Some(1_000_000));
    assert_eq!(r.acs(), Some(7));
    let empty = rec(Position::new(), &[]);
    assert_eq!(
        (empty.dm(), empty.acd(), empty.acn(), empty.acs()),
        (None, None, None, None)
    );
}

#[test]
fn counters_reject_negative_and_non_numeric_text() {
    let r = rec(Position::new(), &[("acd", "-1"), ("acn", "x"), ("acs", "")]);

    assert_eq!(r.acd(), None);
    assert_eq!(r.acn(), None);
    assert_eq!(r.acs(), None);
}

#[test]
fn acn_accepts_values_beyond_u32() {
    let r = rec(Position::new(), &[("acn", "5000000000")]);

    assert_eq!(r.acn(), Some(5_000_000_000));
}

#[test]
fn id_eco_and_comments_come_without_quotes() {
    let r = rec(
        Position::new(),
        &[
            ("id", "\"BK.01\""),
            ("eco", "\"C20\""),
            ("c0", "\"first\""),
            ("c9", "\"last; one\""),
            ("c3", "bare"),
        ],
    );

    assert_eq!(r.id(), Some("BK.01"));
    assert_eq!(r.eco(), Some("C20"));
    assert_eq!(r.comment(0), Some("first"));
    assert_eq!(r.comment(9), Some("last; one"));
    assert_eq!(r.comment(3), Some("bare"));
    assert_eq!(r.comment(1), None, "absent comment");
    assert_eq!(r.comment(10), None, "only c0..c9 exist");
    assert_eq!(r.comment(255), None);
}

#[test]
fn unquoted_keeps_unbalanced_quotes_as_found() {
    assert_eq!(op("id", "\"abc").unquoted(), "\"abc");
    assert_eq!(op("id", "abc\"").unquoted(), "abc\"");
    assert_eq!(op("id", "\"").unquoted(), "\"");
    assert_eq!(op("id", "\"\"").unquoted(), "");
    assert_eq!(op("id", "").unquoted(), "");
}

#[test]
fn operation_new_trims_operands_and_checks_opcode_shape() {
    let o = op("bm", "  e4  ");
    assert_eq!(o.operands, "e4");
    assert!(op("bm", "").has_standard_opcode());
    assert!(op("c0", "").has_standard_opcode());
    assert!(op("draw_offer", "").has_standard_opcode());
    assert!(!op("", "").has_standard_opcode());
    assert!(!op("1x", "").has_standard_opcode());
    assert!(!op("a-b", "").has_standard_opcode());
    assert!(!op("a b", "").has_standard_opcode());
}

#[test]
fn record_set_and_remove_opcode() {
    let mut r = rec(Position::new(), &[("id", "a"), ("bm", "e4"), ("id", "b")]);

    r.set_operation("id", "z");
    assert_eq!(
        pairs(&r),
        [("id", "z"), ("bm", "e4")],
        "duplicates collapse into the first"
    );
    r.set_operation("ce", " 10 ");
    assert_eq!(r.centipawns(), Some(10));
    assert_eq!(r.remove_opcode("bm"), 1);
    assert_eq!(r.remove_opcode("bm"), 0);
    assert_eq!(r.position(), &Position::new());
}

// ---------------------------------------------------------------- 4. writing

#[test]
fn writes_exact_standard_format() {
    let mut book = EpdBook::new();
    book.insert(
        Position::new(),
        vec![op("bm", "e4"), op("id", "\"a ; b\""), op("noop", "")],
    );
    book.insert(after("e4"), vec![]);

    let text = text_of(&book, EpdOptions::default());

    let expected = format!("{START4} bm e4; id \"a ; b\"; noop;\n{E4_4}\n");
    assert_eq!(text, expected);
}

#[test]
fn writes_always_four_fields_and_lf() {
    let load = read(format!("{START4} 7 33 id a;\r\n"));

    let text = text_of(&load.book, EpdOptions::default());

    assert_eq!(text, format!("{START4} id a;\n"));
}

#[test]
fn empty_book_writes_nothing() {
    assert_eq!(text_of(&EpdBook::new(), EpdOptions::default()), "");
}

#[test]
fn writes_en_passant_field_only_when_a_capture_is_possible() {
    let with_pawn = fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3");
    let mut book = EpdBook::new();
    book.insert(after("e4"), vec![]); // no black pawn next to e4: the field is `-`
    book.insert(with_pawn, vec![]);

    let text = text_of(&book, EpdOptions::default());

    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines[0], E4_4);
    assert_eq!(
        lines[1],
        "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6"
    );
}

#[test]
fn write_clocks_adds_hmvc_and_fmvn_after_the_operations() {
    let load = read(format!("{START4} 5 9 id a;\n"));
    let opts = EpdOptions::standard().with_write_clocks(true);

    assert_eq!(
        text_of(&load.book, EpdOptions::default()),
        format!("{START4} id a;\n"),
        "off by default"
    );
    assert_eq!(
        text_of(&load.book, opts),
        format!("{START4} id a; hmvc 5; fmvn 9;\n")
    );
}

#[test]
fn write_clocks_does_not_duplicate_existing_hmvc_or_fmvn() {
    let load = read(format!("{START4} 5 9 hmvc 1; fmvn 2;\n"));
    let opts = EpdOptions::standard().with_write_clocks(true);

    assert_eq!(
        text_of(&load.book, opts),
        format!("{START4} hmvc 1; fmvn 2;\n")
    );
}

#[test]
fn scid_escapes_on_write_match_pbook_write_file() {
    // pbook.cpp:752-768: `\` -> `\\`, `;` -> `\s`; one space before each
    // operation and a `;` after it.
    let book = single_record_book(
        Position::new(),
        vec![op("c0", "a;b\\c"), op("id", "\"q\""), op("noop", "")],
    );

    let text = text_of(&book, scid());

    assert_eq!(text, format!("{START4} c0 a\\sb\\\\c; id \"q\"; noop;\n"));
}

#[test]
fn standard_dialect_does_not_escape_backslashes() {
    let book = single_record_book(Position::new(), vec![op("c0", "\"a\\b\"")]);

    assert_eq!(
        text_of(&book, EpdOptions::default()),
        format!("{START4} c0 \"a\\b\";\n")
    );
}

#[rstest]
#[case::lone_quote("c0", "\"abc")]
#[case::lone_quote_with_semicolon("c0", "\"a;b")]
#[case::three_quotes("c0", "\"a\" \"b")]
fn unbalanced_quote_is_unwritable_in_the_standard_dialect(
    #[case] opcode: &str,
    #[case] operands: &str,
) {
    // Written as is, it would swallow the rest of the line on reading, and
    // closing it would change the operand: the round trip would break.
    let book = single_record_book(Position::new(), vec![op(opcode, operands), op("id", "x")]);

    let result = book.write(Vec::new(), EpdOptions::default());

    assert!(
        matches!(result, Err(EpdError::Unwritable { .. })),
        "{result:?}"
    );
}

#[test]
fn nothing_is_written_when_a_later_record_is_unwritable() {
    let mut book = EpdBook::new();
    book.insert(Position::new(), vec![op("id", "fine")]);
    book.insert(after("e4"), vec![op("c0", "bad;value")]);
    let mut out = Vec::new();

    let result = book.write(&mut out, EpdOptions::default());

    assert!(matches!(result, Err(EpdError::Unwritable { .. })));
    assert!(
        out.is_empty(),
        "no partial output: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn quotes_are_not_closed_in_the_scid_dialect() {
    let book = single_record_book(Position::new(), vec![op("c0", "\"abc")]);

    assert_eq!(text_of(&book, scid()), format!("{START4} c0 \"abc;\n"));
}

#[rstest]
#[case::empty_opcode("", "x")]
#[case::space_in_opcode("a b", "x")]
#[case::tab_in_opcode("a\tb", "x")]
#[case::lf_in_operands("bm", "e4\nid x")]
#[case::cr_in_operands("bm", "e4\rid x")]
#[case::lf_in_opcode("a\nb", "")]
#[case::bare_semicolon("c0", "a;b")]
#[case::semicolon_after_balanced_quotes("c0", "\"a\" ; b")]
#[case::semicolon_in_opcode("a;b", "")]
fn unwritable_operations_are_rejected_in_the_standard_dialect(
    #[case] opcode: &str,
    #[case] operands: &str,
) {
    let book = single_record_book(
        Position::new(),
        vec![EpdOperation {
            opcode: opcode.to_owned(),
            operands: operands.to_owned(),
        }],
    );
    let mut out = Vec::new();

    let result = book.write(&mut out, EpdOptions::default());

    assert!(
        matches!(result, Err(EpdError::Unwritable { .. })),
        "{opcode:?} {operands:?} -> {result:?}"
    );
}

#[rstest]
#[case::empty_opcode("", "x")]
#[case::space_in_opcode("a b", "x")]
#[case::lf_in_operands("bm", "e4\nid x")]
#[case::cr_in_operands("bm", "e4\rid x")]
fn unwritable_operations_are_rejected_in_the_scid_dialect(
    #[case] opcode: &str,
    #[case] operands: &str,
) {
    let book = single_record_book(
        Position::new(),
        vec![EpdOperation {
            opcode: opcode.to_owned(),
            operands: operands.to_owned(),
        }],
    );

    let result = book.write(Vec::new(), scid());

    assert!(
        matches!(result, Err(EpdError::Unwritable { .. })),
        "{result:?}"
    );
}

#[test]
fn semicolon_in_operands_is_writable_in_the_scid_dialect() {
    let book = single_record_book(Position::new(), vec![op("c0", "a;b")]);

    let load = read_scid(text_of(&book, scid()));

    assert_eq!(load.book, book);
}

#[test]
fn unwritable_error_names_the_opcode() {
    let book = single_record_book(Position::new(), vec![op("c0", "a;b")]);

    match book.write(Vec::new(), EpdOptions::default()) {
        Err(EpdError::Unwritable { opcode, .. }) => assert_eq!(opcode, "c0"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn write_reports_io_errors_from_the_sink() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("sink broke"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("sink broke"))
        }
    }
    let book = single_record_book(Position::new(), vec![op("id", "a")]);

    let result = book.write(Broken, EpdOptions::default());

    assert!(
        matches!(result, Err(EpdError::Io { path: None, .. })),
        "{result:?}"
    );
}

#[test]
fn round_trip_read_write_read_gives_an_equal_book() {
    let source = format!(
        "{START4} bm e4 Nf3; id \"x\"; c0 \"a ; b\"; noop;\n{E4_4} ce -25; pv e5 Nf3;\n{D4_4}\n"
    );
    let first = read(&source).book;

    let written = text_of(&first, EpdOptions::default());
    let second = read(&written).book;

    assert_eq!(first, second);
    assert_eq!(
        written, source,
        "canonical input is reproduced byte for byte"
    );
}

#[test]
fn round_trip_in_the_scid_dialect() {
    let source = format!("{START4} c0 a\\sb\\\\c; id z; noop;\n{E4_4} id \u{e9}\u{4e2d};\n");
    let first = read_scid(&source).book;

    let written = text_of(&first, scid());
    let second = read_scid(&written).book;

    assert_eq!(first, second);
    assert_eq!(written, source);
}

#[test]
fn round_trip_keeps_duplicate_opcodes_and_order() {
    let book = single_record_book(
        Position::new(),
        vec![op("id", "a"), op("bm", "e4"), op("id", "b"), op("noop", "")],
    );

    let back = read(text_of(&book, EpdOptions::default())).book;

    assert_eq!(back, book);
}

#[test]
fn round_trip_with_non_standard_opcode() {
    let book = single_record_book(Position::new(), vec![op("1x", "foo"), op("_a", "")]);

    let back = read(text_of(&book, EpdOptions::default())).book;

    assert_eq!(back, book);
}

#[test]
fn write_file_round_trips_and_leaves_no_temporary_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.epd");
    let book = read(format!("{START4} id a;\n{E4_4} bm e5;\n")).book;

    book.write_file(&path, EpdOptions::default())
        .expect("write_file");

    let back = EpdBook::read_file(&path, EpdOptions::default())
        .expect("read")
        .book;
    assert_eq!(back, book);
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, [std::ffi::OsString::from("out.epd")]);
}

#[test]
fn write_file_replaces_an_existing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.epd");
    std::fs::write(&path, "old contents\n").expect("seed");
    let book = single_record_book(Position::new(), vec![op("id", "new")]);

    book.write_file(&path, EpdOptions::default())
        .expect("write_file");

    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        format!("{START4} id new;\n")
    );
}

#[test]
fn write_file_of_an_empty_book_creates_an_empty_file() {
    // `sc_epd create` writes an empty book (tkscid.cpp:3963); rule 18.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("new.epd");

    EpdBook::new()
        .write_file(&path, EpdOptions::default())
        .expect("write_file");

    assert_eq!(std::fs::metadata(&path).expect("exists").len(), 0);
}

#[test]
fn write_file_failure_leaves_the_destination_intact_and_no_temp_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("keep.epd");
    std::fs::write(&path, "precious\n").expect("seed");
    let mut book = EpdBook::new();
    book.insert(Position::new(), vec![op("id", "fine")]);
    book.insert(after("e4"), vec![op("c0", "bad;value")]); // Unwritable

    let result = book.write_file(&path, EpdOptions::default());

    assert!(
        matches!(result, Err(EpdError::Unwritable { .. })),
        "{result:?}"
    );
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "precious\n");
    let count = std::fs::read_dir(dir.path()).expect("dir").count();
    assert_eq!(count, 1, "temporary file was cleaned up");
}

#[test]
fn write_file_into_a_missing_directory_is_an_io_error_with_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no_such_dir").join("out.epd");

    let result = EpdBook::new().write_file(&path, EpdOptions::default());

    match result {
        Err(EpdError::Io { path: Some(p), .. }) => assert_eq!(p, path),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn write_file_to_a_path_without_file_name_fails_cleanly() {
    let result = EpdBook::new().write_file(Path::new(""), EpdOptions::default());

    assert!(matches!(result, Err(EpdError::Io { .. })), "{result:?}");
}

// ---------------------------------------------------------------- 5. property and scale tests

fn random_position(choices: &[u8]) -> Position {
    let mut pos = Position::new();
    for &c in choices {
        let moves = pos.legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[usize::from(c) % moves.len()];
        match pos.after(mv) {
            Ok(next) => pos = next,
            Err(_) => break,
        }
    }
    pos
}

fn standard_operand() -> BoxedStrategy<String> {
    let word = "[a-zA-Z0-9.+\\-]{1,6}";
    let quoted = "\"[a-zA-Z0-9 ;\\\\#.\u{e9}\u{4e2d}]{0,10}\"";
    prop::collection::vec(
        prop_oneof![word.prop_map(String::from), quoted.prop_map(String::from)],
        0..4,
    )
    .prop_map(|parts| parts.join(" "))
    .boxed()
}

fn scid_operand() -> BoxedStrategy<String> {
    "[a-zA-Z0-9 ;\"\\\\.\u{e9}]{0,14}".boxed()
}

fn operations(operand: BoxedStrategy<String>) -> impl Strategy<Value = Vec<EpdOperation>> {
    prop::collection::vec(("[a-z][a-z0-9_]{0,5}", operand), 0..5).prop_map(|v| {
        v.into_iter()
            .map(|(o, a)| EpdOperation::new(o, a))
            .collect()
    })
}

fn book_from(entries: Vec<(Vec<u8>, Vec<EpdOperation>)>) -> EpdBook {
    let mut book = EpdBook::new();
    for (choices, ops) in entries {
        book.insert(random_position(&choices), ops);
    }
    book
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn reader_never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..3000)) {
        for opts in [EpdOptions::default(), scid()] {
            let load = EpdBook::read(bytes.as_slice(), opts).expect("memory reads do not fail");
            prop_assert!(!load.book.is_modified());
        }
    }

    #[test]
    fn reader_never_panics_on_lines_after_a_valid_position(tail in "\\PC{0,200}", crlf in any::<bool>()) {
        let eol = if crlf { "\r\n" } else { "\n" };
        let text = format!("{START4} {tail}{eol}{E4_4} {tail}");
        for opts in [EpdOptions::default(), scid()] {
            let load = EpdBook::read(text.as_bytes(), opts).expect("read");
            prop_assert!(load.book.len() <= 2);
            // What was read can be written or rejected, never panic.
            drop(load.book.write(Vec::new(), opts));
        }
    }

    #[test]
    fn writing_what_was_read_is_stable_after_one_pass(tail in "[a-z0-9 ;\"\\\\]{0,40}") {
        // read -> write -> read -> write must settle (the first write may
        // close an unterminated quote or trim text).
        for opts in [EpdOptions::default(), scid()] {
            let text = format!("{START4} {tail}\n");
            let first = EpdBook::read(text.as_bytes(), opts).expect("read").book;
            let mut w1 = Vec::new();
            if first.write(&mut w1, opts).is_err() {
                continue;
            }
            let second = EpdBook::read(w1.as_slice(), opts).expect("read").book;
            let mut w2 = Vec::new();
            prop_assert!(second.write(&mut w2, opts).is_ok(), "{:?}", String::from_utf8_lossy(&w1));
            prop_assert_eq!(String::from_utf8_lossy(&w1), String::from_utf8_lossy(&w2));
        }
    }

    #[test]
    fn round_trip_of_random_books_in_the_standard_dialect(
        entries in prop::collection::vec((prop::collection::vec(any::<u8>(), 0..24), operations(standard_operand())), 0..8)
    ) {
        let book = book_from(entries);

        let text = text_of(&book, EpdOptions::default());
        let load = EpdBook::read(text.as_bytes(), EpdOptions::default()).expect("read");

        prop_assert!(load.diagnostics.iter().all(|d| !matches!(d.kind, EpdDiagnosticKind::InvalidPosition { .. } | EpdDiagnosticKind::UnterminatedString | EpdDiagnosticKind::DuplicatePosition { .. })), "{:?}", load.diagnostics);
        prop_assert_eq!(load.book, book);
    }

    #[test]
    fn round_trip_of_random_books_in_the_scid_dialect(
        entries in prop::collection::vec((prop::collection::vec(any::<u8>(), 0..24), operations(scid_operand())), 0..8)
    ) {
        let book = book_from(entries);

        let text = text_of(&book, scid());
        let load = EpdBook::read(text.as_bytes(), scid()).expect("read");

        prop_assert_eq!(load.book, book);
    }

    #[test]
    fn insert_then_remove_leaves_the_other_records_in_order(
        keep in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..24), 1..8),
        drop_at in any::<prop::sample::Index>(),
    ) {
        let book = book_from(keep.iter().cloned().map(|c| (c, Vec::new())).collect());
        let live: Vec<Position> = book.iter().map(|r| r.position().clone()).collect();
        let victim = live[drop_at.index(live.len())].clone();
        let mut edited = book.clone();

        edited.remove(&victim);

        let expected: Vec<&Position> = live.iter().filter(|p| **p != victim).collect();
        let actual: Vec<&Position> = edited.iter().map(EpdRecord::position).collect();
        prop_assert_eq!(actual, expected);
        // Cyclic walk from the last record visits each live record once.
        if let Some(mut cursor) = edited.last_index() {
            let mut seen = 0;
            for _ in 0..edited.len() {
                cursor = edited.next(cursor).expect("non-empty");
                seen += 1;
            }
            prop_assert_eq!(seen, edited.len());
            prop_assert_eq!(Some(cursor), edited.last_index(), "full cycle returns to the start");
        }
    }
}

/// Builds `count` distinct legal positions (kings fixed, three white pieces
/// on distinct squares, Black to move).
fn distinct_position_lines(count: usize) -> String {
    let mut out = String::with_capacity(count * 70);
    let free: Vec<usize> = (0..64).filter(|&s| s != 0 && s != 63).collect();
    let mut produced = 0;
    'outer: for &n in &free {
        for &b in &free {
            for &r in &free {
                if n == b || n == r || b == r {
                    continue;
                }
                let mut board = ['1'; 64];
                board[0] = 'K'; // a1 in a8-first order would be wrong; only distinctness matters
                board[63] = 'k';
                board[n] = 'N';
                board[b] = 'B';
                board[r] = 'R';
                let mut fen = String::new();
                for rank in 0..8 {
                    let mut empty = 0;
                    for file in 0..8 {
                        let c = board[rank * 8 + file];
                        if c == '1' {
                            empty += 1;
                        } else {
                            if empty > 0 {
                                fen.push_str(&empty.to_string());
                                empty = 0;
                            }
                            fen.push(c);
                        }
                    }
                    if empty > 0 {
                        fen.push_str(&empty.to_string());
                    }
                    if rank < 7 {
                        fen.push('/');
                    }
                }
                out.push_str(&fen);
                out.push_str(" b - - id \"n\"; bm Ke8;\n");
                produced += 1;
                if produced == count {
                    break 'outer;
                }
            }
        }
    }
    assert_eq!(produced, count, "generator must produce enough positions");
    out
}

#[test]
fn reading_and_writing_one_hundred_thousand_records_is_fast_and_exact() {
    let text = distinct_position_lines(100_000);
    let start = Instant::now();

    let load = read(&text);
    let read_time = start.elapsed();

    let rejected = load
        .diagnostics
        .iter()
        .filter(|d| is_invalid_position(&d.kind))
        .count();
    // Black to move with the black king attacked by a white piece is legal;
    // White's king can never be attacked (Black has only a king), so only
    // touching kings or similar impossibilities may be rejected.
    assert_eq!(
        rejected,
        0,
        "{:?}",
        &load.diagnostics[..load.diagnostics.len().min(3)]
    );
    assert_eq!(load.book.len(), 100_000);
    let first = load.book.iter().next().expect("first");
    assert_eq!(first.id(), Some("n"));

    let w_start = Instant::now();
    let mut out = Vec::new();
    load.book
        .write(&mut out, EpdOptions::default())
        .expect("write");
    let write_time = w_start.elapsed();
    let again = read(&out);
    assert_eq!(again.book, load.book);

    // Generous bounds for unoptimised builds on slow CI: a quadratic
    // implementation would need minutes.
    assert!(read_time.as_secs() < 60, "read took {read_time:?}");
    assert!(write_time.as_secs() < 60, "write took {write_time:?}");
}

#[test]
fn many_duplicate_positions_do_not_blow_up() {
    let mut text = String::new();
    for i in 0..20_000 {
        writeln!(text, "{START4} id n{i};").expect("string write");
    }
    let start = Instant::now();

    let load = read(&text);

    assert_eq!(load.book.len(), 1);
    assert_eq!(load.diagnostics.len(), 19_999);
    assert_eq!(
        load.book.get(&Position::new()).and_then(EpdRecord::id),
        Some("n19999")
    );
    assert!(start.elapsed().as_secs() < 60);
}

// ---------------------------------------------------------------- 6. compat

// The spec says the reference tree ships no `.epd` file, and that holds:
// `reference/scid_vs_pc-4.27` only has `src/eco2epd.cpp` and `tcl/file/epd.tcl`
// (`engines/phalanx/test.fin` is a `FEN/solution` list in another format).
// So there is no `compat_*` test for the EPD; the ECO round trip through
// `EcoBook::to_epd_book` is covered by the ECO tests.
