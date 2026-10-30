// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::mask::save`
// 1957-1965, `::tree::mask::open` 1822-1857). Copyright (c) 1999-2002 Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The mask file (`.stm`, `docs/formatos/stm.md`; decisions D1800 to D1816):
//! reading (tolerant, nothing is executed), writing (atomic) and encoding.

use std::borrow::Cow;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use encoding_rs::{Encoding, WINDOWS_1252};
use mychess_core::Position;
use thiserror::Error;

use super::matching::MoveMatcher;
use super::model::{
    DupIndex, MAX_COMMENT_BYTES, MAX_FIELD_BYTES, MAX_MOVES_PER_POSITION, Marker, MaskColor,
    MaskInsertError, MaskKey, MaskMove, MaskNag, MaskPosition, TreeMask, clean_comment,
};
use super::tcl::{self, ListIter, ScriptError, TclError, push_element, push_word};
use crate::tree::stc::{create_temp, resolve_target};

/// The suffix of a mask file.
pub const STM_SUFFIX: &str = ".stm";
/// A file bigger than this is not read, and one that would be is not
/// written (D1815).
pub const MAX_STM_BYTES: u64 = 256 * 1024 * 1024;
/// The most warnings a [`StmReport`] lists (D1814), plus the one of
/// [`StmWarningKind::TooManyPositions`], which is always listed (last).
pub const MAX_WARNINGS: usize = 100;

const HEADER: &str = "set ::tree::mask::maskSerialized ";

/// Errors of the mask file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StmError {
    /// A file operation failed.
    #[error("i/o error on {path}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// The file is bigger than [`MAX_STM_BYTES`], or the mask would be when
    /// written. The `len` is in bytes: the size of the file for [`read`] and
    /// [`parse`], the size of the encoded list for [`write()`].
    #[error("mask file too large ({len} bytes)")]
    TooLarge {
        /// The size in bytes.
        len: u64,
    },
    /// The destination of a write is read-only (the same answer on every
    /// platform, D1856); the file is left as it is.
    #[error("{path} is read-only")]
    ReadOnly {
        /// The file.
        path: PathBuf,
    },
    /// The text is not a mask file: empty, or it does not start with `set`.
    #[error("not a tree mask file")]
    NotAMask,
    /// The file has the `set` command but the text is malformed (unbalanced
    /// braces, garbage after a word...). Nothing is loaded.
    #[error("syntax error at byte {offset}: {reason}")]
    Syntax {
        /// Approximate byte offset in the (decoded) text.
        offset: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// The file holds something other than the one `set` command (another
    /// command, extra words, a substitution). Nothing is executed or loaded.
    #[error("the file has more than a mask definition (byte {offset}); refused")]
    ForeignCommand {
        /// Byte offset in the (decoded) text.
        offset: usize,
    },
}

/// What a warning of the reader is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StmWarningKind {
    /// The list has an odd number of elements; the last one is ignored.
    OddElementCount,
    /// A key repeated: the last one wins.
    DuplicateKey,
    /// The key is not a short FEN of standard chess: kept, never matched.
    KeyNotFen,
    /// A key or field is longer than [`MAX_FIELD_BYTES`].
    FieldTooLong,
    /// The file has more than [`MAX_POSITIONS`](super::MAX_POSITIONS)
    /// positions: the rest is dropped (D1850).
    TooManyPositions,
    /// The value of a position is not a valid list: the position is dropped.
    BadPositionValue,
    /// The value of a position does not have two elements.
    WrongValueLength,
    /// The list of moves is not valid: the position has no moves.
    BadMoveList,
    /// A move is not a valid list: dropped.
    BadMove,
    /// A move does not have six elements.
    WrongMoveLength,
    /// A move with an empty SAN: dropped.
    EmptySan,
    /// A move repeated in the position (D1805): the second is dropped.
    DuplicateMove,
    /// More than [`MAX_MOVES_PER_POSITION`] moves: the rest is dropped.
    TooManyMoves,
    /// A comment longer than [`MAX_COMMENT_BYTES`]: cut.
    CommentTruncated,
    /// A NAG that is not one of the seven: preserved.
    UnknownNag,
    /// A color that is not one of the five or `#hex`: preserved.
    UnknownColor,
    /// A marker image the Scid does not know: preserved.
    UnknownMarker,
}

impl fmt::Display for StmWarningKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StmWarningKind::OddElementCount => "odd number of elements, last one ignored",
            StmWarningKind::DuplicateKey => "repeated position, the last one is used",
            StmWarningKind::KeyNotFen => "key is not a short FEN, kept as text",
            StmWarningKind::FieldTooLong => "field too long",
            StmWarningKind::TooManyPositions => "too many positions, rest dropped",
            StmWarningKind::BadPositionValue => "malformed position, dropped",
            StmWarningKind::WrongValueLength => "position value without two elements",
            StmWarningKind::BadMoveList => "malformed list of moves",
            StmWarningKind::BadMove => "malformed move, dropped",
            StmWarningKind::WrongMoveLength => "move without six elements",
            StmWarningKind::EmptySan => "move without text, dropped",
            StmWarningKind::DuplicateMove => "repeated move, dropped",
            StmWarningKind::TooManyMoves => "too many moves in a position, rest dropped",
            StmWarningKind::CommentTruncated => "comment too long, cut",
            StmWarningKind::UnknownNag => "unknown NAG, kept",
            StmWarningKind::UnknownColor => "unknown color, kept",
            StmWarningKind::UnknownMarker => "unknown marker, kept",
        })
    }
}

/// A warning of the reader, with the index of the entry of the file it is
/// about (`pair`, 0-based, in file order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StmWarning {
    /// The 0-based index of the position in the file.
    pub pair: usize,
    /// What happened.
    pub kind: StmWarningKind,
}

impl fmt::Display for StmWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "position {}: {}", self.pair + 1, self.kind)
    }
}

/// What the reader found (D1814).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StmReport {
    /// Positions loaded.
    pub positions: usize,
    /// Moves loaded (`null` included).
    pub moves: usize,
    /// The first [`MAX_WARNINGS`] warnings.
    pub warnings: Vec<StmWarning>,
    /// Warnings after the first [`MAX_WARNINGS`], only counted.
    pub dropped_warnings: usize,
}

impl StmReport {
    /// The number of warnings, listed or not.
    pub fn total_warnings(&self) -> usize {
        self.warnings.len() + self.dropped_warnings
    }

    fn warn(&mut self, pair: usize, kind: StmWarningKind) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(StmWarning { pair, kind });
        } else {
            self.dropped_warnings += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Decodes the bytes of a file (D1813): BOMs, valid UTF-8, else
/// Windows-1252; line ends become `\n`.
fn decode(bytes: &[u8]) -> Cow<'_, str> {
    let text: Cow<'_, str> = match Encoding::for_bom(bytes) {
        Some((enc, n)) => {
            let (t, _) = enc.decode_without_bom_handling(bytes.get(n..).unwrap_or_default());
            t
        }
        None => match std::str::from_utf8(bytes) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => WINDOWS_1252.decode_without_bom_handling(bytes).0,
        },
    };
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        text
    }
}

/// Reads a mask from the bytes of a file.
///
/// # Errors
///
/// [`StmError::TooLarge`], [`StmError::NotAMask`], [`StmError::Syntax`] and
/// [`StmError::ForeignCommand`]; content problems become warnings in the
/// report instead.
pub fn parse(bytes: &[u8]) -> Result<(TreeMask, StmReport), StmError> {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > MAX_STM_BYTES {
        return Err(StmError::TooLarge { len });
    }
    let text = decode(bytes);
    let script = tcl::parse_set_script(&text).map_err(|e| match e {
        ScriptError::Empty | ScriptError::NotSet => StmError::NotAMask,
        ScriptError::Foreign { offset } => StmError::ForeignCommand { offset },
        ScriptError::Syntax(e) => StmError::Syntax {
            offset: e.offset,
            reason: e.reason,
        },
    })?;
    let syntax = |e: TclError| StmError::Syntax {
        offset: script.offset + e.offset,
        reason: e.reason,
    };
    let mut report = StmReport::default();
    let mut mask = TreeMask::new();
    // The list is read lazily, pair by pair: a hostile file costs the mask
    // and nothing else (D1850).
    let mut elements = ListIter::new(&script.value);
    let mut n = 0usize;
    while let Some(key_text) = elements.next() {
        let key_text = key_text.map_err(syntax)?;
        let Some(value) = elements.next() else {
            report.warn(n, StmWarningKind::OddElementCount);
            break;
        };
        let value = value.map_err(syntax)?;
        if !read_position(&mut mask, &mut report, n, &key_text, &value) {
            break;
        }
        n += 1;
    }
    report.positions = mask.len();
    report.moves = mask.iter().map(|(_, p)| p.moves.len()).sum();
    Ok((mask, report))
}

/// Reads the mask file at `path`.
///
/// # Errors
///
/// [`StmError::Io`] plus the errors of [`parse`]; a file bigger than
/// [`MAX_STM_BYTES`] is refused before it is read.
pub fn read(path: &Path) -> Result<(TreeMask, StmReport), StmError> {
    let io = |source| StmError::Io {
        path: path.to_path_buf(),
        source,
    };
    let file = File::open(path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    if len > MAX_STM_BYTES {
        return Err(StmError::TooLarge { len });
    }
    let mut bytes = Vec::new();
    // `take` guards against a file that grows while it is read.
    file.take(MAX_STM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    parse(&bytes)
}

/// Reads one position. `false` when the mask is full and reading must stop.
fn read_position(
    mask: &mut TreeMask,
    report: &mut StmReport,
    n: usize,
    key_text: &str,
    value: &str,
) -> bool {
    if key_text.len() > MAX_FIELD_BYTES {
        report.warn(n, StmWarningKind::FieldTooLong);
        return true;
    }
    let key = MaskKey::from_text(key_text);
    if matches!(key, MaskKey::Opaque(_)) {
        report.warn(n, StmWarningKind::KeyNotFen);
    }
    let Ok((parts, count)) = tcl::split_head(value, 2) else {
        report.warn(n, StmWarningKind::BadPositionValue);
        return true;
    };
    if count != 2 {
        report.warn(n, StmWarningKind::WrongValueLength);
    }
    let mut position = MaskPosition::default();
    if let Some(text) = parts.get(1) {
        let (comment, cut) = clean_comment(text);
        if cut {
            report.warn(n, StmWarningKind::CommentTruncated);
        }
        position.comment = comment;
    }
    let pos = key.to_position();
    if let Some(list) = parts.first() {
        read_moves(&mut position, report, n, list, pos.as_ref());
    }
    match mask.insert_raw(key, position) {
        Ok(true) => report.warn(n, StmWarningKind::DuplicateKey),
        Ok(false) => {}
        Err(MaskInsertError::Full) => {
            // Never lost among the counted ones: it is the last warning.
            report.warnings.push(StmWarning {
                pair: n,
                kind: StmWarningKind::TooManyPositions,
            });
            return false;
        }
        Err(_) => report.warn(n, StmWarningKind::FieldTooLong),
    }
    true
}

fn read_moves(
    position: &mut MaskPosition,
    report: &mut StmReport,
    n: usize,
    list: &str,
    pos: Option<&Position>,
) {
    // Checked first, so a broken list gives no moves at all.
    if tcl::count_list(list).is_err() {
        report.warn(n, StmWarningKind::BadMoveList);
        return;
    }
    let matcher = MoveMatcher::new(pos);
    let mut dups = DupIndex::default();
    let mut too_many = false;
    for item in ListIter::new(list).flatten() {
        let Ok((fields, count)) = tcl::split_head(&item, 6) else {
            report.warn(n, StmWarningKind::BadMove);
            continue;
        };
        if count != 6 {
            report.warn(n, StmWarningKind::WrongMoveLength);
        }
        let field = |i: usize| fields.get(i).map_or("", |c| c.as_ref());
        let san = field(0).trim();
        if san.is_empty() {
            report.warn(n, StmWarningKind::EmptySan);
            continue;
        }
        if san.len() > MAX_FIELD_BYTES
            || [1, 2, 4, 5]
                .iter()
                .any(|&i| field(i).len() > MAX_FIELD_BYTES)
        {
            report.warn(n, StmWarningKind::FieldTooLong);
            continue;
        }
        if dups.check_and_add(&position.moves, &matcher, san) {
            report.warn(n, StmWarningKind::DuplicateMove);
            continue;
        }
        if position.moves.len() >= MAX_MOVES_PER_POSITION {
            too_many = true;
            continue;
        }
        position.moves.push(read_move(report, n, san, &fields));
    }
    if too_many {
        report.warn(n, StmWarningKind::TooManyMoves);
    }
}

fn read_move(report: &mut StmReport, n: usize, san: &str, fields: &[Cow<'_, str>]) -> MaskMove {
    let field = |i: usize| fields.get(i).map_or("", |c| c.as_ref());
    let nag = MaskNag::from_text(field(1));
    if matches!(nag, Some(MaskNag::Other(_))) {
        report.warn(n, StmWarningKind::UnknownNag);
    }
    let color = MaskColor::from_text(field(2));
    if matches!(color, MaskColor::Other(_)) && !color.is_hex() {
        report.warn(n, StmWarningKind::UnknownColor);
    }
    let (comment, cut) = clean_comment(field(3));
    if cut {
        report.warn(n, StmWarningKind::CommentTruncated);
    }
    let markers = [Marker::from_text(field(4)), Marker::from_text(field(5))];
    if markers.iter().any(|m| matches!(m, Some(Marker::Other(_)))) {
        report.warn(n, StmWarningKind::UnknownMarker);
    }
    MaskMove {
        san: san.to_owned(),
        nag,
        color,
        comment,
        markers,
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Cleans a comment for writing: no `\r`, trimmed, no longer than the limit.
fn comment_for_file(text: &str) -> Cow<'_, str> {
    let clean = !text.contains('\r')
        && text.len() <= MAX_COMMENT_BYTES
        && text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}')) == text;
    if clean {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(clean_comment(text).0)
    }
}

fn encode_move(m: &MaskMove) -> String {
    let mut s = String::with_capacity(m.san.len() + m.comment.len() + 64);
    push_element(&mut s, &m.san);
    push_element(&mut s, m.nag.as_ref().map_or("", MaskNag::as_str));
    push_element(&mut s, m.color.as_str());
    push_element(&mut s, &comment_for_file(&m.comment));
    for marker in &m.markers {
        push_element(&mut s, marker.as_ref().map_or("", Marker::file_name));
    }
    s
}

/// The list `V` of the file: pairs of key and position (`stm.md` section 2).
fn encode_list(mask: &TreeMask) -> String {
    let mut v = String::new();
    for (key, position) in mask.iter() {
        if position.is_dead() {
            continue;
        }
        let mut moves = String::new();
        for m in position.moves.iter().filter(|m| !m.san.trim().is_empty()) {
            push_element(&mut moves, &encode_move(m));
        }
        let mut value = String::new();
        push_element(&mut value, &moves);
        push_element(&mut value, &comment_for_file(&position.comment));
        push_element(&mut v, key.as_str());
        push_element(&mut v, &value);
    }
    v
}

fn write_script<W: Write>(w: &mut W, list: &str) -> std::io::Result<u64> {
    let mut word = String::new();
    push_word(&mut word, list);
    w.write_all(HEADER.as_bytes())?;
    w.write_all(word.as_bytes())?;
    w.write_all(b"\n")?;
    Ok((HEADER.len() + word.len() + 1) as u64)
}

/// The text of the mask file: `set ::tree::mask::maskSerialized {...}` and a
/// newline, in file order (D1812).
pub fn serialize(mask: &TreeMask) -> String {
    let mut out = Vec::new();
    // Writing to a `Vec` cannot fail.
    write_script(&mut out, &encode_list(mask)).ok();
    String::from_utf8(out).unwrap_or_default()
}

/// What [`write_report`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StmWrite {
    /// The size of the file.
    pub size: u64,
    /// Whether the permissions of the replaced file were copied to the new
    /// one. `false` is a warning, not an error (D1856): the content is
    /// written, only the mode may differ.
    pub permissions_kept: bool,
}

/// Writes the mask to `path` atomically (D1816): a temporary file in the same
/// directory, flushed, synced and closed before the `rename` that replaces
/// the destination, keeping its permissions; a symbolic link is followed.
/// Returns the size of the file. See [`write_report`] for the permission
/// warning.
///
/// # Errors
///
/// [`StmError::TooLarge`] when the encoded list would pass
/// [`MAX_STM_BYTES`] (its `len` is in bytes); [`StmError::ReadOnly`] when the
/// destination exists and is read-only (the Scid would fail too, and the
/// answer is the same on Unix and Windows); [`StmError::Io`] otherwise. The
/// old file stays and the temporary one is removed.
pub fn write(path: &Path, mask: &TreeMask) -> Result<u64, StmError> {
    write_report(path, mask).map(|w| w.size)
}

/// Like [`write()`], also telling whether the permissions were kept.
///
/// # Errors
///
/// The same as [`write()`].
pub fn write_report(path: &Path, mask: &TreeMask) -> Result<StmWrite, StmError> {
    let list = encode_list(mask);
    if list.len() as u64 > MAX_STM_BYTES {
        return Err(StmError::TooLarge {
            len: list.len() as u64,
        });
    }
    let dest = resolve_target(path);
    let io = |source| StmError::Io {
        path: dest.clone(),
        source,
    };
    let permissions = match fs::metadata(&dest) {
        Ok(meta) => Some(meta.permissions()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io(e)),
    };
    if permissions.as_ref().is_some_and(fs::Permissions::readonly) {
        return Err(StmError::ReadOnly { path: dest });
    }
    let (tmp, file) = create_temp(&dest).map_err(io)?;
    let result = (|| {
        let mut out = BufWriter::with_capacity(1 << 16, file);
        let size = write_script(&mut out, &list)?;
        out.flush()?;
        let file = out
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)?;
        let permissions_kept = match permissions {
            Some(p) => file.set_permissions(p).is_ok(),
            None => true,
        };
        file.sync_all()?;
        // Closed here: the Windows rename needs it.
        drop(file);
        fs::rename(&tmp, &dest)?;
        sync_parent(&dest);
        Ok(StmWrite {
            size,
            permissions_kept,
        })
    })();
    match result {
        Ok(w) => Ok(w),
        Err(e) => {
            fs::remove_file(&tmp).ok();
            Err(io(e))
        }
    }
}

/// Makes the rename durable. Best effort, and only where a directory can be
/// opened and synced (Unix); Windows has no equivalent.
fn sync_parent(dest: &Path) {
    #[cfg(unix)]
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(handle) = File::open(dir)
    {
        handle.sync_all().ok();
    }
    #[cfg(not(unix))]
    let _ = dest;
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_A: &str = "set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{null {} white {} {} {}} {e4 ! Green {King's pawn} ::rep::_tb_include {}} {d4 {} white {} {} {}}} {My repertoire}} {rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -} {{{e5 {} white {} {} {}} {c5 ?! Red {Sicilian: see line 2} ::rep::_tb_exclude ::tree::mask::imageMainLine}} {}} {rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -} {{{Nf3 !? Yellow {Line one\nsecond line} tb_bkm {}}} {}}}\n";

    #[test]
    fn example_a_is_read_and_written_back_identically() {
        let text = EXAMPLE_A;
        let (mask, report) = parse(text.as_bytes()).unwrap();
        assert_eq!(report.total_warnings(), 0, "{:?}", report.warnings);
        assert_eq!((report.positions, report.moves), (3, 6));
        let (_, start) = mask.iter().next().unwrap();
        assert_eq!(start.comment, "My repertoire");
        assert_eq!(start.moves[1].nag, Some(MaskNag::Good));
        assert_eq!(start.moves[1].color, MaskColor::Green);
        assert_eq!(start.moves[1].comment, "King's pawn");
        assert_eq!(start.moves[1].markers[0], Some(Marker::Include));
        assert!(start.moves[0].is_null());
        assert_eq!(serialize(&mask), text);
    }

    #[test]
    fn example_b_comments_round_trip() {
        let mut mask = TreeMask::new();
        let mut p = MaskPosition::default();
        for (san, c) in [
            ("e4", "a { b"),
            ("d4", "x\\"),
            ("c4", "a\\\nb"),
            ("Nf3", "q\"[$];# '"),
        ] {
            let mut m = MaskMove::new(san);
            m.comment = c.into();
            p.moves.push(m);
        }
        mask.insert(MaskKey::from_position(&Position::new()), p);
        let text = serialize(&mask);
        assert!(text.contains("{e4 {} white a\\ \\{\\ b {} {}}"), "{text}");
        assert!(text.contains("{d4 {} white x\\\\ {} {}}"), "{text}");
        assert!(text.contains("{c4 {} white a\\\\\\nb {} {}}"), "{text}");
        let (back, report) = parse(text.as_bytes()).unwrap();
        assert_eq!(report.total_warnings(), 0);
        assert_eq!(back, mask);
    }

    #[test]
    fn empty_mask_round_trips() {
        let text = serialize(&TreeMask::new());
        assert_eq!(text, "set ::tree::mask::maskSerialized {}\n");
        let (mask, report) = parse(text.as_bytes()).unwrap();
        assert!(mask.is_empty());
        assert_eq!(report, StmReport::default());
    }

    #[test]
    fn dead_positions_are_not_written() {
        let mut mask = TreeMask::new();
        let mut p = MaskPosition::default();
        p.moves.push(MaskMove::new("null"));
        mask.insert(MaskKey::from_position(&Position::new()), p);
        assert_eq!(serialize(&mask), "set ::tree::mask::maskSerialized {}\n");
    }

    #[test]
    fn refuses_what_is_not_a_mask() {
        assert!(matches!(parse(b""), Err(StmError::NotAMask)));
        assert!(matches!(parse(b"  \n# nothing\n"), Err(StmError::NotAMask)));
        assert!(matches!(
            parse(&[0, 1, 2, 255, 254]),
            Err(StmError::NotAMask)
        ));
        assert!(matches!(
            parse(b"source /etc/passwd"),
            Err(StmError::NotAMask)
        ));
        for bad in [
            "set ::tree::mask::maskSerialized {}\nexec rm -rf /\n",
            "set ::tree::mask::maskSerialized {}; source x.tcl\n",
            "set ::tree::mask::maskSerialized [exec x]\n",
            "set other {}\n",
        ] {
            assert!(
                matches!(parse(bad.as_bytes()), Err(StmError::ForeignCommand { .. })),
                "{bad}"
            );
        }
        assert!(matches!(
            parse(b"set ::tree::mask::maskSerialized {{a b}"),
            Err(StmError::Syntax { .. })
        ));
    }

    #[test]
    fn unknown_values_survive_a_round_trip() {
        let text = "set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{e4 +- #00ff00 {} tb_zzz {}}} {}} {opaque key} {{{x {} white {} {} {}}} {}}}\n";
        let (mask, _) = parse(text.as_bytes()).unwrap();
        assert_eq!(serialize(&mask), text);
    }

    #[test]
    fn encodings_and_line_ends() {
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";
        let body = format!(
            "set ::tree::mask::maskSerialized {{{{{start}}} {{{{{{e4 {{}} white {{caf\u{e9}\nline}} {{}} {{}}}}}} {{}}}}}}\n"
        );
        let comment = "caf\u{e9}\nline";
        for bytes in [
            body.clone().into_bytes(),
            [&[0xef, 0xbb, 0xbf][..], body.as_bytes()].concat(),
            body.replace('\n', "\r\n").into_bytes(),
            body.replace('\n', "\r").into_bytes(),
            // Windows-1252: the e acute is the single byte 0xE9.
            body.chars()
                .map(|c| if c == '\u{e9}' { 0xe9 } else { c as u8 })
                .collect(),
        ] {
            let (mask, _) = parse(&bytes).unwrap_or_else(|e| panic!("{e}: {bytes:?}"));
            let (_, p) = mask.iter().next().unwrap();
            assert_eq!(p.moves[0].comment, comment, "{bytes:?}");
        }
        let (mask, _) = parse(body.as_bytes()).unwrap();
        let out = serialize(&mask);
        assert!(!out.contains('\r') && !out.starts_with('\u{feff}'));
    }

    #[test]
    fn write_is_atomic_and_keeps_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.stm");
        let mut mask = TreeMask::new();
        mask.apply(
            &Position::new(),
            &super::super::MaskOp::AddMove { san: "e4".into() },
        );
        let size = write(&path, &mask).unwrap();
        assert_eq!(size, fs::metadata(&path).unwrap().len());
        assert_eq!(read(&path).unwrap().0, mask);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            write(&path, &TreeMask::new()).unwrap();
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o640);
        }
        let left: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(left.len(), 1, "no temporary file is left");
        assert!(matches!(
            read(&dir.path().join("missing.stm")),
            Err(StmError::Io { .. })
        ));
    }
}
