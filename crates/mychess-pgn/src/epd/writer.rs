//! Writing EPD text.

use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use super::book::EpdBook;
use super::error::EpdError;
use super::reader::{EpdOptions, MAX_LINE_BYTES, is_counter, split_operations};
use super::record::{EpdOperation, EpdRecord};

impl EpdBook {
    /// Writes the book, one line per record in book order.
    ///
    /// Each line is the four-field FEN followed by ` opcode operands;` for
    /// every operation (nothing else for a record without operations), then
    /// `\n`. Reading the output with the same [`EpdOptions`] gives back an
    /// equal book.
    ///
    /// An operation that cannot be expressed (an opcode empty or with white
    /// space, a line break anywhere, a `;` outside quotes or an odd number of
    /// `"` in the standard dialect) is an [`EpdError::Unwritable`]. The whole
    /// book is checked before the first byte is written, so in that case
    /// nothing at all is written. A record whose line would exceed the
    /// reader's limit (1 MiB, which `read_file` reports as `LineTooLong`) is
    /// also unwritable, so a save never silently loses a record.
    /// Operands are trimmed.
    pub fn write<W: Write>(&self, writer: W, options: EpdOptions) -> Result<(), EpdError> {
        self.validate(options)?;
        self.write_unchecked(writer, options)
    }

    /// Checks that every record can be written.
    fn validate(&self, options: EpdOptions) -> Result<(), EpdError> {
        let mut line = String::new();
        for record in self.iter() {
            line.clear();
            format_record(record, options, &mut line)?;
        }
        Ok(())
    }

    fn write_unchecked<W: Write>(&self, writer: W, options: EpdOptions) -> Result<(), EpdError> {
        let io_err = |source| EpdError::Io { path: None, source };
        let mut out = BufWriter::new(writer);
        let mut line = String::new();
        for record in self.iter() {
            line.clear();
            format_record(record, options, &mut line)?;
            out.write_all(line.as_bytes()).map_err(io_err)?;
        }
        out.flush().map_err(io_err)
    }

    /// Writes the book to `path` atomically: the data goes to a temporary
    /// file in the same directory, which then replaces the target (keeping
    /// its permissions). A symbolic link is followed and its target is
    /// replaced. A book with no records gives an empty file. On any failure
    /// the target is left untouched and no temporary file remains.
    ///
    /// Writing compressed output is not supported: a `.gz` path is refused
    /// with [`EpdError::Unsupported`]; see [`EpdBook::write_file_atomic`].
    pub fn write_file(&self, path: impl AsRef<Path>, options: EpdOptions) -> Result<(), EpdError> {
        let path = path.as_ref();
        if is_gz_name(path) {
            return Err(EpdError::Unsupported {
                path: path.to_owned(),
                reason: "writing gzip is not supported",
            });
        }
        self.write_file_to(path, options, false)
    }

    /// Like [`EpdBook::write_file`], but a path whose extension is `gz`
    /// (any case) is written gzip-compressed, which
    /// [`EpdBook::read_file`] reads back by its magic bytes. Same atomic
    /// guarantees: temporary file in the same directory, `rename` over the
    /// target, original untouched and no `.tmp` left on failure. Records
    /// keep their order (holes left by `remove` are simply not written).
    ///
    /// Spec M7-5a (D3806).
    pub fn write_file_atomic(
        &self,
        path: impl AsRef<Path>,
        options: EpdOptions,
    ) -> Result<(), EpdError> {
        let path = path.as_ref();
        self.write_file_to(path, options, is_gz_name(path))
    }

    fn write_file_to(&self, path: &Path, options: EpdOptions, gzip: bool) -> Result<(), EpdError> {
        let io_err = |source| EpdError::Io {
            path: Some(path.to_owned()),
            source,
        };
        if path.file_name().is_none() {
            return Err(io_err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path has no file name",
            )));
        }
        self.validate(options)?;

        let is_link = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
        let target: PathBuf = if is_link {
            fs::canonicalize(path).map_err(io_err)?
        } else {
            path.to_owned()
        };
        let dir = match target.parent() {
            Some(d) if !d.as_os_str().is_empty() => d,
            _ => Path::new("."),
        };

        let tmp = new_temp_file(dir).map_err(io_err)?;
        let written = if gzip {
            let mut encoder =
                flate2::write::GzEncoder::new(tmp.as_file(), flate2::Compression::default());
            self.write_unchecked(&mut encoder, options).and_then(|()| {
                encoder
                    .finish()
                    .map(drop)
                    .map_err(|source| EpdError::Io { path: None, source })
            })
        } else {
            self.write_unchecked(tmp.as_file(), options)
        };
        written.map_err(|e| match e {
            EpdError::Io { source, .. } => io_err(source),
            other => other,
        })?;
        if let Ok(meta) = fs::metadata(&target) {
            tmp.as_file()
                .set_permissions(meta.permissions())
                .map_err(io_err)?;
        }
        tmp.as_file().sync_all().map_err(io_err)?;
        // Close the handle before renaming (Windows refuses otherwise); the
        // `TempPath` still deletes the file if the rename fails.
        let tmp_path = tmp.into_temp_path();
        tmp_path.persist(&target).map_err(|e| io_err(e.error))
    }
}

fn is_gz_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gz"))
}

/// Creates the temporary file next to the target.
///
/// On Unix `tempfile` defaults to mode 0600; request 0o666 instead, which the
/// kernel masks with the umask at `open(2)`, matching what `fs::write` gives a
/// new file. An existing target's permissions are copied over afterwards.
fn new_temp_file(dir: &Path) -> io::Result<tempfile::NamedTempFile> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o666))
            .tempfile_in(dir)
    }
    #[cfg(not(unix))]
    {
        tempfile::NamedTempFile::new_in(dir)
    }
}

fn format_record(
    record: &EpdRecord,
    options: EpdOptions,
    out: &mut String,
) -> Result<(), EpdError> {
    let fen = record.position().fen();
    let mut fields = fen.split_ascii_whitespace().take(4);
    if let Some(first) = fields.next() {
        out.push_str(first);
    }
    for field in fields {
        out.push(' ');
        out.push_str(field);
    }
    for (i, op) in record.operations().iter().enumerate() {
        out.push(' ');
        format_operation(op, i == 0, options, out)?;
        out.push(';');
    }
    if options.write_clocks {
        for (opcode, value) in [
            ("hmvc", record.position().halfmove_clock()),
            ("fmvn", record.position().fullmove_number()),
        ] {
            if record.operation(opcode).is_none() {
                out.push(' ');
                out.push_str(opcode);
                out.push(' ');
                out.push_str(&value.to_string());
                out.push(';');
            }
        }
    }
    // The reader drops lines longer than its limit (`LineTooLong`), so such a
    // record would vanish on reopening: refuse to write it.
    if out.len() > MAX_LINE_BYTES {
        return Err(EpdError::Unwritable {
            opcode: record
                .operations()
                .iter()
                .max_by_key(|op| op.opcode.len() + op.operands.len())
                .map_or_else(String::new, |op| op.opcode.clone()),
            reason: "record line exceeds the reader's line length limit",
        });
    }
    out.push('\n');
    Ok(())
}

/// Appends `opcode operands` (without the final `;`).
fn format_operation(
    op: &EpdOperation,
    first: bool,
    options: EpdOptions,
    out: &mut String,
) -> Result<(), EpdError> {
    let fail = |reason| EpdError::Unwritable {
        opcode: op.opcode.clone(),
        reason,
    };
    if op.opcode.is_empty() || op.opcode.contains(char::is_whitespace) {
        return Err(fail("opcode is empty or contains white space"));
    }
    let operands = op.operands.trim();
    // Right after the four FEN fields, two numeric tokens would be read back
    // as the halfmove clock and fullmove number instead of an operation.
    if first
        && is_counter(&op.opcode)
        && operands
            .split_once(char::is_whitespace)
            .is_some_and(|(head, _)| is_counter(head))
    {
        return Err(fail("would be read back as the clock counters"));
    }
    if [&op.opcode, operands]
        .iter()
        .any(|t| t.contains(['\n', '\r']))
    {
        return Err(fail("line break in operation"));
    }
    let mut text = String::with_capacity(op.opcode.len() + operands.len() + 2);
    text.push_str(&op.opcode);
    if !operands.is_empty() {
        text.push(' ');
        text.push_str(operands);
    }
    if options.scid_escapes {
        for c in text.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                ';' => out.push_str("\\s"),
                _ => out.push(c),
            }
        }
        return Ok(());
    }
    // Standard dialect: nothing is escaped, so the text must survive the
    // reader's splitting as exactly one operation.
    if text.matches('"').count() % 2 == 1 {
        return Err(fail("unbalanced quote"));
    }
    if ops_roundtrip(&text, &op.opcode, operands) {
        out.push_str(&text);
        Ok(())
    } else {
        Err(fail("semicolon outside quotes"))
    }
}

/// True when `text` reads back as the single operation (`opcode`, `operands`).
fn ops_roundtrip(text: &str, opcode: &str, operands: &str) -> bool {
    let mut ops = split_operations(text, false, |_| {});
    match (ops.pop(), ops.is_empty()) {
        (Some(o), true) => o.opcode == opcode && o.operands == operands,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::{Position, SanMode};
    use proptest::prelude::*;
    use std::fs::File;

    fn op(o: &str, a: &str) -> EpdOperation {
        EpdOperation::new(o, a)
    }

    fn text(book: &EpdBook, options: EpdOptions) -> String {
        let mut buf = Vec::new();
        book.write(&mut buf, options).expect("write");
        String::from_utf8(buf).expect("utf-8")
    }

    fn after(san: &str) -> Position {
        let p = Position::new();
        let m = p.parse_san(san, SanMode::Strict).expect("legal").mv;
        p.after(m).expect("legal")
    }

    #[test]
    fn line_format() {
        let mut b = EpdBook::new();
        b.insert(
            Position::new(),
            vec![op("bm", "e4"), op("id", "\"a ; b\""), op("noop", "")],
        );
        b.insert(after("e4"), vec![]);
        assert_eq!(
            text(&b, EpdOptions::default()),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - bm e4; id \"a ; b\"; noop;\n\
             rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -\n"
        );
    }

    #[test]
    fn empty_book_writes_nothing() {
        assert_eq!(text(&EpdBook::new(), EpdOptions::default()), "");
    }

    #[test]
    fn scid_escapes_on_write() {
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![op("c0", "a;b\\c")]);
        let o = EpdOptions::scid();
        assert!(text(&b, o).ends_with(" c0 a\\sb\\\\c;\n"));
        let back = EpdBook::read(text(&b, o).as_bytes(), o).expect("read").book;
        assert_eq!(back, b);
    }

    #[test]
    fn write_clocks_adds_missing_operations_only() {
        let mut b = EpdBook::new();
        let p = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 3 7")
            .expect("fen");
        b.insert(p, vec![op("fmvn", "9")]);
        let o = EpdOptions::standard().with_write_clocks(true);
        assert!(text(&b, o).ends_with(" fmvn 9; hmvc 3;\n"));
        assert!(text(&b, EpdOptions::default()).ends_with(" fmvn 9;\n"));
    }

    #[test]
    fn first_operation_that_looks_like_clocks_is_unwritable() {
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![op("0", "0 a")]);
        let mut buf = Vec::new();
        assert!(b.write(&mut buf, EpdOptions::default()).is_err());
        // Later in the line it is plain text and round-trips.
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![op("id", "x"), op("0", "0 a")]);
        let t = text(&b, EpdOptions::default());
        let back = EpdBook::read(t.as_bytes(), EpdOptions::default()).expect("read");
        assert_eq!(back.book, b);
    }

    #[test]
    fn unwritable_operations() {
        for bad in [
            op("", "x"),
            op("a b", "x"),
            op("a", "x\ny"),
            op("a", "x ; y"),
        ] {
            let mut b = EpdBook::new();
            b.insert(Position::new(), vec![bad]);
            let mut buf = Vec::new();
            assert!(
                matches!(
                    b.write(&mut buf, EpdOptions::default()),
                    Err(EpdError::Unwritable { .. })
                ),
                "{b:?}"
            );
        }
    }

    #[test]
    fn odd_quotes_are_unwritable_and_nothing_is_written() {
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![op("id", "ok")]);
        b.insert(after("e4"), vec![op("c0", "\"open")]);
        let mut buf = Vec::new();
        let err = b
            .write(&mut buf, EpdOptions::default())
            .expect_err("odd quote");
        assert!(matches!(
            err,
            EpdError::Unwritable {
                reason: "unbalanced quote",
                ..
            }
        ));
        assert!(buf.is_empty(), "no partial output");
        // The Scid dialect does not track quotes, so it can write them.
        let o = EpdOptions::scid();
        let back = EpdBook::read(text(&b, o).as_bytes(), o).expect("read").book;
        assert_eq!(back, b);
    }

    #[test]
    fn read_write_read_round_trip() {
        let input = "\
r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - bm Bb5; id \"ruy\"; c0 \"a ; semicolon in a string\";
rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 noop; c0 \"\"; c0 b; c0 a;
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -
";
        let opts = EpdOptions::default();
        let first = EpdBook::read(input.as_bytes(), opts).expect("read").book;
        let written = text(&first, opts);
        let second = EpdBook::read(written.as_bytes(), opts).expect("read").book;
        assert_eq!(first, second);
        assert_eq!(text(&second, opts), written);
        assert_eq!(first.len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn write_file_new_file_is_not_owner_only_and_existing_keeps_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let b = EpdBook::default();
        let new = dir.path().join("new.epd");
        b.write_file(&new, EpdOptions::default()).expect("write");
        let mode = fs::metadata(&new).expect("meta").permissions().mode() & 0o777;
        // Same as `fs::write` would give: 0o666 & !umask.
        let reference = dir.path().join("ref.txt");
        fs::write(&reference, b"").expect("write ref");
        let expected = fs::metadata(&reference).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, expected, "new file mode must follow the umask");

        let old = dir.path().join("old.epd");
        fs::write(&old, b"x").expect("write old");
        fs::set_permissions(&old, fs::Permissions::from_mode(0o640)).expect("chmod");
        b.write_file(&old, EpdOptions::default()).expect("write");
        let mode = fs::metadata(&old).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn write_file_is_atomic_and_readable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("book.epd");
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![op("id", "x")]);
        b.write_file(&path, EpdOptions::default()).expect("write");
        EpdBook::new()
            .write_file(dir.path().join("empty.epd"), EpdOptions::default())
            .expect("write");
        assert_eq!(
            fs::metadata(dir.path().join("empty.epd"))
                .expect("meta")
                .len(),
            0
        );
        let back = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
        assert_eq!(back.book, b);
        // Only the two files remain (no temporary leftovers).
        assert_eq!(fs::read_dir(dir.path()).expect("dir").count(), 2);
        // Overwrite.
        EpdBook::new()
            .write_file(&path, EpdOptions::default())
            .expect("overwrite");
        assert_eq!(fs::metadata(&path).expect("meta").len(), 0);
    }

    #[test]
    fn gz_files_are_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("book.epd.gz");
        let mut enc = flate2::write::GzEncoder::new(
            File::create(&path).expect("create"),
            flate2::Compression::default(),
        );
        enc.write_all(b"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - id gz;\n")
            .expect("write");
        enc.finish().expect("finish");
        let l = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
        assert_eq!(
            l.book.get(&Position::new()).and_then(EpdRecord::id),
            Some("gz")
        );
        assert!(matches!(
            EpdBook::read_file(dir.path().join("missing.epd"), EpdOptions::default()),
            Err(EpdError::Io { path: Some(_), .. })
        ));
    }

    #[test]
    fn failed_write_file_leaves_target_and_directory_intact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("book.epd");
        let mut good = EpdBook::new();
        good.insert(Position::new(), vec![op("id", "old")]);
        good.write_file(&path, EpdOptions::default())
            .expect("write");
        let before = fs::read(&path).expect("read");
        let mut bad = EpdBook::new();
        bad.insert(Position::new(), vec![op("id", "ok")]);
        bad.insert(after("e4"), vec![op("c0", "\"open")]);
        assert!(matches!(
            bad.write_file(&path, EpdOptions::default()),
            Err(EpdError::Unwritable { .. })
        ));
        assert_eq!(fs::read(&path).expect("read"), before);
        assert_eq!(fs::read_dir(dir.path()).expect("dir").count(), 1);
    }

    #[test]
    fn failed_rename_removes_the_temporary() {
        let dir = tempfile::tempdir().expect("tempdir");
        // The target is a non-empty directory: the rename must fail.
        let target = dir.path().join("book.epd");
        fs::create_dir(&target).expect("mkdir");
        fs::write(target.join("x"), b"x").expect("write");
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![]);
        assert!(matches!(
            b.write_file(&target, EpdOptions::default()),
            Err(EpdError::Io { path: Some(_), .. })
        ));
        assert_eq!(fs::read_dir(dir.path()).expect("dir").count(), 1);
    }

    #[test]
    fn write_file_with_scid_escapes_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.epd");
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![op("c0", "a;b\\c \"q")]);
        let o = EpdOptions::scid();
        b.write_file(&path, o).expect("write");
        assert_eq!(EpdBook::read_file(&path, o).expect("read").book, b);
    }

    #[test]
    fn gz_target_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("b.epd.GZ");
        assert!(matches!(
            EpdBook::new().write_file(&path, EpdOptions::default()),
            Err(EpdError::Unsupported { .. })
        ));
        assert!(!path.exists());
    }

    #[test]
    fn gzip_content_is_detected_whatever_the_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("book.epd");
        let mut enc = flate2::write::GzEncoder::new(
            File::create(&path).expect("create"),
            flate2::Compression::default(),
        );
        enc.write_all(b"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - id gz;\n")
            .expect("write");
        enc.finish().expect("finish");
        let l = EpdBook::read_file(&path, EpdOptions::default()).expect("read");
        assert_eq!(
            l.book.get(&Position::new()).and_then(EpdRecord::id),
            Some("gz")
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_file_follows_symlinks_and_keeps_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("real.epd");
        let link = dir.path().join("link.epd");
        fs::write(&real, b"").expect("write");
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).expect("chmod");
        symlink(&real, &link).expect("symlink");
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![op("id", "x")]);
        b.write_file(&link, EpdOptions::default()).expect("write");
        assert!(
            fs::symlink_metadata(&link)
                .expect("meta")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            EpdBook::read_file(&real, EpdOptions::default())
                .expect("read")
                .book,
            b
        );
        let mode = fs::metadata(&real).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    #[test]
    fn long_file_names_do_not_overflow_the_temporary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{}.epd", "n".repeat(240)));
        EpdBook::new()
            .write_file(&path, EpdOptions::default())
            .expect("write");
        assert!(path.exists());
    }

    /// A valid position reached by random legal moves from the start.
    fn position() -> impl Strategy<Value = Position> {
        prop::collection::vec(any::<prop::sample::Index>(), 0..12).prop_map(|picks| {
            let mut p = Position::new();
            for pick in picks {
                let moves = p.legal_moves();
                if moves.is_empty() {
                    break;
                }
                let m = moves[pick.index(moves.len())];
                match p.after(m) {
                    Ok(next) => p = next,
                    Err(_) => break,
                }
            }
            p
        })
    }

    /// Operands with balanced quotes (semicolons only inside them), Unicode
    /// and tabs.
    fn operand() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                "[a-zA-Z0-9 \t\u{e9}\u{4e2d}\u{1f600}]{0,6}",
                "\"[a-zA-Z0-9 ;\\\\\t\u{e9}\u{4e2d}]{0,6}\"",
            ],
            0..4,
        )
        .prop_map(|parts| parts.concat().trim().to_owned())
    }

    fn book_strategy() -> impl Strategy<Value = EpdBook> {
        prop::collection::vec(
            (
                position(),
                prop::collection::vec(("[a-z][a-z0-9_]{0,5}", operand()), 0..5),
            ),
            1..=5,
        )
        .prop_map(|records| {
            let mut b = EpdBook::new();
            for (p, ops) in records {
                b.insert(p, ops.iter().map(|(o, a)| op(o, a)).collect());
            }
            b
        })
    }

    proptest! {
        #[test]
        fn round_trip_standard(b in book_strategy()) {
            let opts = EpdOptions::default();
            let t = text(&b, opts);
            let back = EpdBook::read(t.as_bytes(), opts).expect("read").book;
            prop_assert_eq!(&back, &b);
            prop_assert_eq!(text(&back, opts), t);
        }

        #[test]
        fn round_trip_scid(
            ops in prop::collection::vec(("[a-z][a-z0-9_]{0,5}", "[a-zA-Z0-9 ;\"\\\\\t\u{e9}]{0,12}"), 0..6)
        ) {
            let mut b = EpdBook::new();
            b.insert(Position::new(), ops.iter().map(|(o, a)| op(o, a)).collect());
            let opts = EpdOptions::scid();
            let t = text(&b, opts);
            let back = EpdBook::read(t.as_bytes(), opts).expect("read").book;
            prop_assert_eq!(back, b);
        }

        #[test]
        fn odd_quotes_are_never_written_in_standard(
            pre in "[a-z ]{0,4}", post in "[a-z ]{0,4}"
        ) {
            let mut b = EpdBook::new();
            b.insert(Position::new(), vec![op("c0", &format!("{pre}\"{post}"))]);
            let mut buf = Vec::new();
            let r = b.write(&mut buf, EpdOptions::default());
            let unwritable = matches!(r, Err(EpdError::Unwritable { .. }));
            prop_assert!(unwritable);
            prop_assert!(buf.is_empty());
        }
    }
}
