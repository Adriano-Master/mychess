//! Reading EPD text: lines, positions and operations.

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use mychess_core::Position;

use super::book::EpdBook;
use super::error::{EpdDiagnostic, EpdDiagnosticKind, EpdError};
use super::record::{EpdOperation, is_standard_opcode};

/// Longest line read, in bytes; longer lines are skipped.
pub(crate) const MAX_LINE_BYTES: usize = 1 << 20;

/// Diagnostics kept per read; further ones are only counted.
const MAX_DIAGNOSTICS: usize = 100_000;

/// Default cap on the decompressed size of a gzip input.
const DEFAULT_MAX_DECOMPRESSED_BYTES: u64 = 256 << 20;

/// Options shared by reading and writing.
///
/// Build with [`EpdOptions::standard`] or [`EpdOptions::scid`] and the
/// `with_*` methods; new options may be added without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct EpdOptions {
    /// Use the Scid dialect: `\s` stands for `;` and `\\` for `\` inside the
    /// operations (any `\x` reads as `x`), and quotes are not tracked, so
    /// every unescaped `;` ends an operation. When writing, `\` and `;` are
    /// escaped. Off by default: standard EPD, where `;` inside double quotes
    /// is text and the backslash is literal.
    pub scid_escapes: bool,
    /// When writing, add `hmvc` and `fmvn` operations from the position's
    /// clocks if the record has none. Off by default.
    pub write_clocks: bool,
    /// When reading, fail with [`EpdError::LimitExceeded`] if the book would
    /// hold more distinct positions than this. `None` (the default) means no
    /// limit.
    pub max_records: Option<usize>,
    /// When reading gzip data, fail with [`EpdError::LimitExceeded`] if it
    /// inflates to more bytes than this (a guard against decompression
    /// bombs). Default: 256 MiB; `None` means no limit. Plain input is not
    /// affected.
    pub max_decompressed_bytes: Option<u64>,
}

impl Default for EpdOptions {
    fn default() -> Self {
        EpdOptions::standard()
    }
}

impl EpdOptions {
    /// Standard EPD: no escapes, quotes are tracked, no clock operations,
    /// no record limit, 256 MiB decompression limit.
    pub const fn standard() -> EpdOptions {
        EpdOptions {
            scid_escapes: false,
            write_clocks: false,
            max_records: None,
            max_decompressed_bytes: Some(DEFAULT_MAX_DECOMPRESSED_BYTES),
        }
    }

    /// The Scid dialect (see [`scid_escapes`](EpdOptions::scid_escapes));
    /// everything else as in [`standard`](EpdOptions::standard).
    pub const fn scid() -> EpdOptions {
        EpdOptions {
            scid_escapes: true,
            ..EpdOptions::standard()
        }
    }

    /// Sets [`scid_escapes`](EpdOptions::scid_escapes).
    #[must_use]
    pub const fn with_scid_escapes(mut self, on: bool) -> EpdOptions {
        self.scid_escapes = on;
        self
    }

    /// Sets [`write_clocks`](EpdOptions::write_clocks).
    #[must_use]
    pub const fn with_write_clocks(mut self, on: bool) -> EpdOptions {
        self.write_clocks = on;
        self
    }

    /// Sets [`max_records`](EpdOptions::max_records).
    #[must_use]
    pub const fn with_max_records(mut self, max: Option<usize>) -> EpdOptions {
        self.max_records = max;
        self
    }

    /// Sets [`max_decompressed_bytes`](EpdOptions::max_decompressed_bytes).
    #[must_use]
    pub const fn with_max_decompressed_bytes(mut self, max: Option<u64>) -> EpdOptions {
        self.max_decompressed_bytes = max;
        self
    }
}

/// The result of reading an EPD file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EpdLoad {
    /// The book (not marked as modified).
    pub book: EpdBook,
    /// Problems found, in line order (at most 100 000 are kept).
    pub diagnostics: Vec<EpdDiagnostic>,
    /// Diagnostics beyond the limit, which were counted but not kept.
    pub dropped_diagnostics: u64,
}

impl EpdBook {
    /// Reads an EPD book from `reader`, line by line.
    ///
    /// Lines end with `\n`, `\r\n` or `\r`; the last one needs no terminator.
    /// Blank lines and lines starting with `#` or `%` are ignored. Text that
    /// is not valid UTF-8 is read as windows-1252. Gzip data is detected by
    /// its magic bytes (`1F 8B`) and inflated on the fly, within
    /// [`EpdOptions::max_decompressed_bytes`]. Bad lines never fail the
    /// read: they become [`EpdDiagnostic`]s. Only an I/O error or a
    /// configured limit does.
    pub fn read<R: Read>(reader: R, options: EpdOptions) -> Result<EpdLoad, EpdError> {
        read_impl(reader, options)
    }

    /// Reads an EPD file. Gzip content is detected by its magic bytes, not
    /// by the file name.
    pub fn read_file(path: impl AsRef<Path>, options: EpdOptions) -> Result<EpdLoad, EpdError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| EpdError::Io {
            path: Some(path.to_owned()),
            source,
        })?;
        read_impl(file, options).map_err(|e| match e {
            EpdError::Io { path: None, source } => EpdError::Io {
                path: Some(path.to_owned()),
                source,
            },
            other => other,
        })
    }
}

/// The marker carried by the `io::Error` of a [`LimitedReader`].
#[derive(Debug)]
struct LimitHit(u64);

impl std::fmt::Display for LimitHit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "more than {} decompressed bytes", self.0)
    }
}

impl std::error::Error for LimitHit {}

/// Fails with [`LimitHit`] once more than `max` bytes have been read.
struct LimitedReader<R> {
    inner: R,
    left: Option<u64>,
    max: u64,
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(left) = self.left else {
            return self.inner.read(buf);
        };
        // Ask for one byte more than allowed, to notice the excess.
        let cap = usize::try_from(left.saturating_add(1)).map_or(buf.len(), |c| c.min(buf.len()));
        let n = self.inner.read(buf.get_mut(..cap).unwrap_or_default())?;
        let n64 = u64::try_from(n).unwrap_or(u64::MAX);
        if n64 > left {
            return Err(io::Error::other(LimitHit(self.max)));
        }
        self.left = Some(left - n64);
        Ok(n)
    }
}

fn io_to_epd(e: io::Error) -> EpdError {
    if let Some(&LimitHit(max)) = e.get_ref().and_then(|r| r.downcast_ref::<LimitHit>()) {
        EpdError::LimitExceeded {
            what: "decompressed bytes",
            max,
        }
    } else {
        EpdError::Io {
            path: None,
            source: e,
        }
    }
}

/// True when the stream starts with the gzip magic bytes (nothing consumed).
fn starts_with_gzip<R: BufRead>(reader: &mut R) -> io::Result<bool> {
    loop {
        match reader.fill_buf() {
            Ok(buf) => return Ok(buf.starts_with(&[0x1F, 0x8B])),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn read_impl<R: Read>(reader: R, options: EpdOptions) -> Result<EpdLoad, EpdError> {
    let mut buffered = BufReader::with_capacity(1 << 16, reader);
    if starts_with_gzip(&mut buffered).map_err(io_to_epd)? {
        let limited = LimitedReader {
            inner: flate2::bufread::MultiGzDecoder::new(buffered),
            left: options.max_decompressed_bytes,
            max: options.max_decompressed_bytes.unwrap_or(u64::MAX),
        };
        read_lines(BufReader::with_capacity(1 << 16, limited), options)
    } else {
        read_lines(buffered, options)
    }
}

fn read_lines<R: BufRead>(reader: R, options: EpdOptions) -> Result<EpdLoad, EpdError> {
    let mut lines = LineReader::new(reader);
    let mut book = EpdBook::new();
    let mut sink = Sink::default();
    // Line where the record of each slot first appeared (slots only grow
    // while reading).
    let mut first_lines: Vec<u64> = Vec::new();
    let mut line_no = 0_u64;

    while let Some(too_long) = lines.next_line().map_err(io_to_epd)? {
        line_no += 1;
        if too_long {
            sink.push(line_no, EpdDiagnosticKind::LineTooLong);
            continue;
        }
        let bytes = if line_no == 1 {
            lines
                .buf
                .strip_prefix(b"\xEF\xBB\xBF")
                .unwrap_or(&lines.buf)
        } else {
            &lines.buf[..]
        };
        let text = decode(bytes);
        let Some((position, operations)) = parse_line(&text, line_no, options, &mut sink) else {
            continue;
        };
        if let Some(max) = options.max_records
            && book.len() >= max
            && !book.contains(&position)
        {
            return Err(EpdError::LimitExceeded {
                what: "records",
                max: u64::try_from(max).unwrap_or(u64::MAX),
            });
        }
        let (slot, added) = book.insert_inner(position, operations);
        if added {
            first_lines.push(line_no);
        } else if let Some(&first_line) = first_lines.get(slot) {
            sink.push(line_no, EpdDiagnosticKind::DuplicatePosition { first_line });
        }
    }
    Ok(EpdLoad {
        book,
        diagnostics: sink.items,
        dropped_diagnostics: sink.dropped,
    })
}

#[derive(Default)]
struct Sink {
    items: Vec<EpdDiagnostic>,
    dropped: u64,
}

impl Sink {
    fn push(&mut self, line: u64, kind: EpdDiagnosticKind) {
        if self.items.len() < MAX_DIAGNOSTICS {
            self.items.push(EpdDiagnostic { line, kind });
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

fn decode(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Cow::Borrowed(s),
        Err(_) => {
            encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(bytes)
                .0
        }
    }
}

/// Splits a byte stream into lines (`\n`, `\r\n`, `\r`), keeping at most
/// [`MAX_LINE_BYTES`] of each.
struct LineReader<R: BufRead> {
    inner: R,
    skip_lf: bool,
    buf: Vec<u8>,
}

impl<R: BufRead> LineReader<R> {
    fn new(inner: R) -> Self {
        LineReader {
            inner,
            skip_lf: false,
            buf: Vec::new(),
        }
    }

    /// Reads the next line into `self.buf`. `Some(true)` means the line was
    /// longer than the limit (and `buf` holds only its start).
    fn next_line(&mut self) -> io::Result<Option<bool>> {
        self.buf.clear();
        let mut too_long = false;
        let mut any = false;
        loop {
            let chunk = match self.inner.fill_buf() {
                Ok(c) => c,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if chunk.is_empty() {
                return Ok(any.then_some(too_long));
            }
            let mut start = 0;
            if self.skip_lf {
                self.skip_lf = false;
                if chunk.first() == Some(&b'\n') {
                    start = 1;
                }
            }
            let rest = chunk.get(start..).unwrap_or_default();
            if let Some(p) = rest.iter().position(|&b| b == b'\n' || b == b'\r') {
                let term = rest.get(p).copied();
                push_bounded(
                    &mut self.buf,
                    rest.get(..p).unwrap_or_default(),
                    &mut too_long,
                );
                self.inner.consume(start + p + 1);
                self.skip_lf = term == Some(b'\r');
                return Ok(Some(too_long));
            }
            if !rest.is_empty() {
                any = true;
                push_bounded(&mut self.buf, rest, &mut too_long);
            }
            let n = chunk.len();
            self.inner.consume(n);
        }
    }
}

fn push_bounded(buf: &mut Vec<u8>, bytes: &[u8], too_long: &mut bool) {
    let room = MAX_LINE_BYTES.saturating_sub(buf.len());
    if bytes.len() > room {
        *too_long = true;
    }
    buf.extend_from_slice(bytes.get(..bytes.len().min(room)).unwrap_or_default());
}

/// Takes the next whitespace-delimited token; returns it and the remainder.
fn next_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some(s.split_at(end))
}

pub(crate) fn is_counter(token: &str) -> bool {
    (1..=9).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_digit())
}

/// Parses one line (without terminator) into a position and operations.
/// `None` for comments, blank lines and unreadable positions.
fn parse_line(
    line: &str,
    line_no: u64,
    options: EpdOptions,
    sink: &mut Sink,
) -> Option<(Position, Vec<EpdOperation>)> {
    let line = line.trim_start();
    if line.is_empty() || line.starts_with(['#', '%']) {
        return None;
    }
    let mut fields: Vec<&str> = Vec::with_capacity(6);
    let mut rest = line;
    for _ in 0..4 {
        let Some((token, after)) = next_token(rest) else {
            break;
        };
        fields.push(token);
        rest = after;
    }
    let counters = if fields.len() == 4 {
        next_token(rest).and_then(|(hm, tail)| next_token(tail).map(|(fm, tail)| (hm, fm, tail)))
    } else {
        None
    };
    if let Some((hm, fm, tail)) = counters
        && is_counter(hm)
        && is_counter(fm)
    {
        fields.push(hm);
        fields.push(fm);
        rest = tail;
    }
    let position = match Position::from_fen(&fields.join(" ")) {
        Ok(p) => p,
        Err(error) => {
            sink.push(line_no, EpdDiagnosticKind::InvalidPosition { error });
            return None;
        }
    };
    let operations = split_operations(rest, options.scid_escapes, |kind| sink.push(line_no, kind));
    Some((position, operations))
}

/// Splits the operation text of a line. A `;` ends an operation unless it
/// is inside double quotes (standard dialect) or escaped (Scid dialect).
pub(crate) fn split_operations(
    text: &str,
    scid_escapes: bool,
    mut report: impl FnMut(EpdDiagnosticKind),
) -> Vec<EpdOperation> {
    let mut ops = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            ';' if scid_escapes || !in_quote => {
                finish_operation(&mut current, &mut ops, &mut report);
            }
            '\\' if scid_escapes => match chars.next() {
                Some('s') => current.push(';'),
                Some(other) => current.push(other),
                None => {}
            },
            '"' if !scid_escapes => {
                in_quote = !in_quote;
                current.push(c);
            }
            _ => current.push(c),
        }
    }
    if in_quote {
        report(EpdDiagnosticKind::UnterminatedString);
    }
    finish_operation(&mut current, &mut ops, &mut report);
    ops
}

fn finish_operation(
    current: &mut String,
    ops: &mut Vec<EpdOperation>,
    report: &mut impl FnMut(EpdDiagnosticKind),
) {
    let text = current.trim();
    if !text.is_empty() {
        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        let (opcode, operands) = text.split_at(end);
        if !is_standard_opcode(opcode) {
            report(EpdDiagnosticKind::NonStandardOpcode {
                opcode: opcode.to_owned(),
            });
        }
        ops.push(EpdOperation::new(opcode, operands));
    }
    current.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::SanMode;

    const START4: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";

    fn load(text: &[u8]) -> EpdLoad {
        EpdBook::read(text, EpdOptions::default()).expect("in-memory read")
    }

    fn scid() -> EpdOptions {
        EpdOptions::scid()
    }

    fn pairs(r: &super::super::EpdRecord) -> Vec<(&str, &str)> {
        r.operations()
            .iter()
            .map(|o| (o.opcode.as_str(), o.operands.as_str()))
            .collect()
    }

    #[test]
    fn standard_line() {
        let l = load(format!("{START4} bm e4 Nf3; id \"start\"; noop;\n").as_bytes());
        assert_eq!(l.diagnostics.len(), 0);
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(
            pairs(r),
            [("bm", "e4 Nf3"), ("id", "\"start\""), ("noop", "")]
        );
        assert!(!l.book.is_modified());
    }

    #[test]
    fn semicolon_inside_quotes_is_text() {
        let l = load(format!("{START4} c0 \"a ; semicolon\"; id x;\n").as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(pairs(r), [("c0", "\"a ; semicolon\""), ("id", "x")]);
    }

    #[test]
    fn backslash_is_literal_in_the_standard_dialect() {
        let l = load(format!("{START4} c0 \"a\\sb\";\n").as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(pairs(r), [("c0", "\"a\\sb\"")]);
    }

    #[test]
    fn scid_escapes() {
        let text = format!("{START4} c0 a\\sb \\\\ \\q; id z;\n");
        let l = EpdBook::read(text.as_bytes(), scid()).expect("read");
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(pairs(r), [("c0", "a;b \\ q"), ("id", "z")]);
        // The Scid dialect splits at any unescaped semicolon, quotes or not.
        let text = format!("{START4} c0 \"a;b\";\n");
        let l = EpdBook::read(text.as_bytes(), scid()).expect("read");
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(pairs(r), [("c0", "\"a"), ("b\"", "")]);
    }

    #[test]
    fn last_line_without_newline_is_read() {
        let l = load(format!("{START4} id a;").as_bytes());
        assert_eq!(l.book.len(), 1);
    }

    #[test]
    fn line_endings() {
        let two = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3";
        for sep in ["\n", "\r\n", "\r"] {
            let text = format!("{START4} id a;{sep}{two} id b;{sep}");
            let l = load(text.as_bytes());
            assert_eq!(l.book.len(), 2, "{sep:?}");
            assert_eq!(l.diagnostics.len(), 0);
        }
        // Blank lines of every kind are silent and counted for line numbers.
        let text = format!("\r\n\n\r{START4} id a;\n{START4} id b;\n");
        let l = load(text.as_bytes());
        assert_eq!(
            l.diagnostics,
            [EpdDiagnostic {
                line: 5,
                kind: EpdDiagnosticKind::DuplicatePosition { first_line: 4 }
            }]
        );
    }

    #[test]
    fn crlf_split_across_buffers() {
        // A one-byte buffer puts `\r` and `\n` in different fills.
        let data = format!("{START4} id a;\r\n\r\n{START4} id b;\r\n");
        let mut lines = LineReader::new(BufReader::with_capacity(1, data.as_bytes()));
        let mut got = Vec::new();
        while lines.next_line().expect("read").is_some() {
            got.push(String::from_utf8(lines.buf.clone()).expect("utf-8"));
        }
        assert_eq!(
            got,
            [
                format!("{START4} id a;"),
                String::new(),
                format!("{START4} id b;")
            ]
        );
    }

    #[test]
    fn duplicate_position_last_wins_first_place() {
        let two = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3";
        let text = format!("{START4} id a;\n{two} id b;\n{START4} id c;\n");
        let l = load(text.as_bytes());
        assert_eq!(l.book.len(), 2);
        let first = l.book.iter().next().expect("first");
        assert_eq!(first.id(), Some("c"));
        assert_eq!(
            l.diagnostics,
            [EpdDiagnostic {
                line: 3,
                kind: EpdDiagnosticKind::DuplicatePosition { first_line: 1 }
            }]
        );
    }

    #[test]
    fn invalid_position_and_comments() {
        let text = format!("# c\n% c\n\nnot a fen\n{START4} id a;\n   # indented comment\n");
        let l = load(text.as_bytes());
        assert_eq!(l.book.len(), 1);
        assert_eq!(l.diagnostics.len(), 1);
        assert_eq!(l.diagnostics[0].line, 4);
        assert!(matches!(
            l.diagnostics[0].kind,
            EpdDiagnosticKind::InvalidPosition { .. }
        ));
    }

    #[test]
    fn six_field_fen_sets_the_clocks() {
        let text = format!("{START4} 7 12 id a;\n");
        let l = load(text.as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(r.position().halfmove_clock(), 7);
        assert_eq!(r.position().fullmove_number(), 12);
        assert_eq!(pairs(r), [("id", "a")]);
        assert_eq!(l.diagnostics.len(), 0);
        // Four fields only, and no operations at all.
        let l = load(START4.as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(r.operations().len(), 0);
    }

    #[test]
    fn empty_operands_and_stray_semicolons() {
        let l = load(format!("{START4} ;; noop;  bm ;  ; id  \"x  y\" ;\n").as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(pairs(r), [("noop", ""), ("bm", ""), ("id", "\"x  y\"")]);
    }

    #[test]
    fn duplicate_opcodes_are_kept_in_order() {
        let l = load(format!("{START4} c0 a; c0 b; bm e4; c0 c;\n").as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(
            pairs(r),
            [("c0", "a"), ("c0", "b"), ("bm", "e4"), ("c0", "c")]
        );
        assert_eq!(r.operations_named("c0").count(), 3);
    }

    #[test]
    fn unterminated_string_runs_to_end_of_line() {
        let l = load(format!("{START4} c0 \"open; id x;\n").as_bytes());
        assert_eq!(l.diagnostics[0].line, 1);
        assert_eq!(l.diagnostics[0].kind, EpdDiagnosticKind::UnterminatedString);
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(r.operations().len(), 1);
        assert_eq!(r.operations()[0].operands, "\"open; id x;");
    }

    #[test]
    fn non_standard_opcode_is_kept_with_a_warning() {
        let l = load(format!("{START4} 9x foo; ok 1;\n").as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(pairs(r), [("9x", "foo"), ("ok", "1")]);
        assert_eq!(
            l.diagnostics[0].kind,
            EpdDiagnosticKind::NonStandardOpcode {
                opcode: "9x".to_owned()
            }
        );
    }

    #[test]
    fn latin1_and_bom() {
        let mut data = b"\xEF\xBB\xBF".to_vec();
        data.extend_from_slice(START4.as_bytes());
        data.extend_from_slice(b" c0 \"caf\xE9\";\n");
        let l = load(&data);
        let r = l.book.get(&Position::new()).expect("record");
        assert_eq!(r.comment(0), Some("caf\u{e9}"));
    }

    #[test]
    fn long_lines_are_skipped_and_reading_goes_on() {
        let mut data = vec![b'x'; MAX_LINE_BYTES + 10];
        data.push(b'\n');
        data.extend_from_slice(START4.as_bytes());
        let l = load(&data);
        assert_eq!(l.book.len(), 1);
        assert_eq!(l.diagnostics[0].kind, EpdDiagnosticKind::LineTooLong);
        assert_eq!(l.diagnostics[0].line, 1);
    }

    #[test]
    fn bm_resolves_against_the_record_position() {
        let l = load(format!("{START4} bm Nf3;\n").as_bytes());
        let r = l.book.get(&Position::new()).expect("record");
        let m = r.bm().expect("legal")[0];
        assert_eq!(
            Position::new()
                .parse_san("Nf3", SanMode::Strict)
                .expect("san")
                .mv,
            m
        );
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let junk: Vec<u8> = (0..=255_u8).cycle().take(5000).collect();
        drop(load(&junk));
        drop(load(b"\xff\xfe\"\"\"\\;;\\"));
        drop(EpdBook::read(&b"\\"[..], scid()));
    }

    #[test]
    fn line_length_boundary() {
        let prefix = format!("{START4} id ");
        let mut ok = prefix.clone().into_bytes();
        ok.resize(MAX_LINE_BYTES, b'x');
        ok.push(b'\n');
        let mut long = prefix.into_bytes();
        long.resize(MAX_LINE_BYTES + 1, b'x');
        long.push(b'\n');
        let l = load(&ok);
        assert_eq!(l.book.len(), 1);
        assert_eq!(l.diagnostics.len(), 0);
        let l = load(&long);
        assert_eq!(l.book.len(), 0);
        assert_eq!(l.diagnostics[0].kind, EpdDiagnosticKind::LineTooLong);
    }

    #[test]
    fn diagnostics_are_capped() {
        let data = "x\n".repeat(MAX_DIAGNOSTICS + 3);
        let l = load(data.as_bytes());
        assert_eq!(l.diagnostics.len(), MAX_DIAGNOSTICS);
        assert_eq!(l.dropped_diagnostics, 3);
        assert_eq!(
            l.diagnostics[MAX_DIAGNOSTICS - 1].line,
            MAX_DIAGNOSTICS as u64
        );
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data).expect("gz");
        enc.finish().expect("finish")
    }

    #[test]
    fn gzip_is_detected_by_magic_bytes_in_read() {
        let l = load(&gzip(format!("{START4} id gz;\n").as_bytes()));
        assert_eq!(
            l.book.get(&Position::new()).and_then(|r| r.id()),
            Some("gz")
        );
    }

    #[test]
    fn decompression_limit() {
        let data = gzip(format!("{START4} id gz;\n").as_bytes());
        let tight = EpdOptions::standard().with_max_decompressed_bytes(Some(10));
        assert!(matches!(
            EpdBook::read(&data[..], tight),
            Err(EpdError::LimitExceeded { max: 10, .. })
        ));
        let exact = EpdOptions::standard()
            .with_max_decompressed_bytes(Some(format!("{START4} id gz;\n").len() as u64));
        assert_eq!(EpdBook::read(&data[..], exact).expect("fits").book.len(), 1);
        let none = EpdOptions::standard().with_max_decompressed_bytes(None);
        assert_eq!(EpdBook::read(&data[..], none).expect("fits").book.len(), 1);
    }

    #[test]
    fn record_limit_counts_distinct_positions() {
        let two = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3";
        let o = EpdOptions::standard().with_max_records(Some(1));
        let same = format!("{START4} id a;\n{START4} id b;\n");
        assert_eq!(
            EpdBook::read(same.as_bytes(), o).expect("one").book.len(),
            1
        );
        let text = format!("{START4} id a;\n{two} id b;\n");
        assert!(matches!(
            EpdBook::read(text.as_bytes(), o),
            Err(EpdError::LimitExceeded {
                what: "records",
                max: 1
            })
        ));
    }

    #[test]
    fn io_error_is_reported() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("boom"))
            }
        }
        assert!(matches!(
            EpdBook::read(Failing, EpdOptions::default()),
            Err(EpdError::Io { path: None, .. })
        ));
    }
}
