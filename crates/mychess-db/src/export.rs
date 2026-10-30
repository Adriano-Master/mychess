// Behaviour derived from Scid vs PC 4.27, src/tkscid.cpp (sc_base_export,
// 1400-1630) and tcl/end.tcl (exportGames).
// Copyright (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Export of games to a PGN file (`sc_base export`, decisions D356 to D360
//! of `docs/specs/m3-3-filtro-busca-importacao.md`).
//!
//! [`export_pgn`] takes the games as an iterator of [`ExportItem`]s, in the
//! order the caller wants (the database order, D360), and writes them with
//! the PGN writer of `mychess-pgn`, one blank line after each game.
//!
//! - A new file is written to `<name>.tmp` in the same directory and renamed
//!   over the destination at the end, so the destination is never half
//!   written (D358). The text before the games is written first and the text
//!   after them last.
//! - With [`ExportOptions::append`] the games go to the end of an existing
//!   file, in the encoding that file already uses; the start text is not
//!   repeated.
//! - Cancelling is a normal end: the games written stay and the end text is
//!   still written.
//! - A game that cannot be read or rendered is skipped and listed in
//!   [`ExportReport::skipped`].

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mychess_core::Game;
use mychess_pgn::write::{Encoding, Newline, WriteOptions, game_to_string};

use crate::progress::{CancelToken, ProgressSink, Ticker};
use crate::si4::Si4Error;
use crate::source::GameSource;

pub mod csv;
pub mod document;
pub mod epd;
pub mod game_list;
pub mod names;
mod text_file;
pub mod viewer;

/// Games between two progress reports (as the import, D329).
const REPORT_EVERY: u32 = 100;
/// Buffer of the output file.
const OUTPUT_BUFFER: usize = 256 * 1024;
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Options of [`export_pgn`] (decision D356).
///
/// Start from [`ExportOptions::scid_dialog`] (the defaults of the Scid
/// export dialog) or [`ExportOptions::default`] (strict PGN) and change the
/// fields.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportOptions {
    /// How each game is written. Its `encoding` field is ignored: the
    /// encoding of the file is [`ExportOptions::encoding`].
    pub pgn: WriteOptions,
    /// Add the games to the end of an existing file instead of creating a
    /// new one.
    ///
    /// The encoding of the file is detected by reading it from the start, so
    /// the cost grows with its size: a file that is valid UTF-8 is read to
    /// the end (a few seconds per GiB), one that is not stops at the first
    /// invalid byte.
    pub append: bool,
    /// Text written before the first game of a new file (not in append
    /// mode), as given.
    pub start_text: String,
    /// Text written after the last game, as given; also when cancelled and
    /// in append mode.
    pub end_text: String,
    /// The encoding of a new file. In append mode the encoding of the
    /// existing file is used (BOM: UTF-8 with BOM; else UTF-8 if valid; else
    /// Latin-1); this one only applies to an empty file.
    pub encoding: Encoding,
    /// Number of games expected, for the `total` of the progress reports.
    /// By default it is taken from the `size_hint` of the iterator
    /// (`0` when unknown).
    pub total: Option<u64>,
    /// When cancelled while writing a new file, throw the temporary file
    /// away and leave the destination as it was (the report still says
    /// `interrupted`). Without it a cancelled export is a normal end that
    /// keeps the games written. Append mode ignores this: the games already
    /// added to the file cannot be taken back.
    pub discard_on_cancel: bool,
    /// When no game was written (all of them were skipped, or there were
    /// none) while writing a new file, throw the temporary file away and
    /// leave the destination as it was, instead of replacing it with an
    /// empty file. Append mode never changes the file in that case anyway.
    pub discard_if_empty: bool,
}

impl Default for ExportOptions {
    /// Strict PGN (`WriteOptions::default`), UTF-8 without BOM, new file.
    fn default() -> ExportOptions {
        ExportOptions {
            pgn: WriteOptions::default(),
            append: false,
            start_text: String::new(),
            end_text: String::new(),
            encoding: Encoding::Utf8,
            total: None,
            discard_on_cancel: false,
            discard_if_empty: false,
        }
    }
}

impl ExportOptions {
    /// The defaults of the export dialog of the Scid (`tcl/start.tcl:838-850`):
    /// comments, variations, a space after the move number, indented
    /// variations, line breaks at 75 columns, numeric NAGs, null moves turned
    /// into comments, no `ScidFlags`. The encoding is UTF-8 without BOM, not
    /// the Latin-1 of the Scid (decision D357).
    pub fn scid_dialog() -> ExportOptions {
        ExportOptions {
            pgn: WriteOptions::scid_export(),
            ..ExportOptions::default()
        }
    }
}

/// One game to export.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExportItem {
    /// A game in memory (the current game, with its unsaved edits).
    Game(Box<Game>),
    /// Game `number` (base 1) of a database, as decoded; an error means the
    /// game is unreadable and will be skipped.
    Stored {
        /// The number of the game in its database.
        number: u32,
        /// The game, or why it could not be read.
        game: Result<Box<Game>, Si4Error>,
    },
}

impl ExportItem {
    /// An item for game `number` of `db`, decoded now.
    pub fn stored<D: GameSource + ?Sized>(db: &mut D, number: u32) -> ExportItem {
        // The game decoded from the data file carries no index flags: the
        // editable ones travel as an extra `ScidFlags` tag, which the writer
        // emits only when asked to (D393).
        let game = db.game(number).map(|mut game| {
            if game.tags().get_extra("ScidFlags").is_none()
                && let Ok(entry) = db.entry(number)
            {
                let letters = entry.flags.letters();
                if !letters.is_empty() {
                    game.tags_mut().set_extra("ScidFlags", &letters).ok();
                }
            }
            Box::new(game)
        });
        ExportItem::Stored { number, game }
    }
}

/// Why a game was not exported.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportSkipReason {
    /// The game could not be read or decoded; the message is the error.
    Unreadable(String),
    /// The game could not be rendered as PGN.
    Unwritable(String),
}

/// The result of [`export_pgn`] (decision D356).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportReport {
    /// The file written (the destination with `.pgn` added when needed).
    pub path: PathBuf,
    /// Games written.
    pub written: u64,
    /// Games not written: the number in the database (0 for a game in
    /// memory) and why.
    pub skipped: Vec<(u32, ExportSkipReason)>,
    /// The export was cancelled; the games before are in the file.
    pub interrupted: bool,
    /// Characters that Latin-1 cannot hold and that became `?`.
    pub unrepresentable: u64,
    /// Time spent.
    pub elapsed: Duration,
}

/// Failure of an export that is not about one game.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// The destination has no file name.
    #[error("{path} has no file name")]
    NoFileName {
        /// The destination.
        path: PathBuf,
    },
    /// An operating system error on a file.
    #[error("i/o error on {path}")]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// Append mode (HTML, LaTeX): the file has no final line (`</body>` or
    /// `\end{document}`) to insert the games before. Nothing was changed.
    #[error("{path} has no line {marker} to append before")]
    NoAppendMarker {
        /// The file.
        path: PathBuf,
        /// The line that was looked for.
        marker: &'static str,
    },
    /// Append mode (HTML, LaTeX): the file is not valid UTF-8, so it cannot
    /// be rewritten safely. Nothing was changed.
    #[error("{path} is not valid UTF-8")]
    NotUtf8 {
        /// The file.
        path: PathBuf,
    },
    /// No game contributed to the output (an empty selection, or an EPD
    /// export where no position was chosen). No file was created.
    #[error("nothing to export")]
    NothingToExport,
    /// Games were handled but none could be exported (the viewer export:
    /// every one was unreadable or unwritable). No file was created.
    #[error("none of the {handled} games could be exported ({skipped} skipped)")]
    AllSkipped {
        /// Games taken from the input.
        handled: u64,
        /// Games left out (the same as `handled` unless a cancel came first).
        skipped: u64,
        /// The first game left out, with its number (0 if none) and reason.
        first: Option<(u32, ExportSkipReason)>,
    },
    /// The game list format has no known field letter.
    #[error("the game list format has no known field")]
    EmptyFormat,
    /// An option is outside its range (the text says which).
    #[error("invalid export option: {0}")]
    InvalidOptions(&'static str),
}

/// `path` with `.pgn` added unless the name already ends in `.pgn`, with any
/// letter case (`X.PGN` stays; Scid made it `X.PGN.pgn`, decision D359).
pub fn with_pgn_extension(path: &Path) -> PathBuf {
    // A path without a file name (`dir/`, `..`) is returned as it is.
    let Some(file_name) = path.file_name() else {
        return path.to_owned();
    };
    if file_name.to_string_lossy().to_lowercase().ends_with(".pgn") {
        return path.to_owned();
    }
    let mut name: OsString = file_name.to_owned();
    name.push(".pgn");
    path.with_file_name(name)
}

fn io_error(path: &Path) -> impl Fn(io::Error) -> ExportError + '_ {
    move |source| ExportError::Io {
        path: path.to_owned(),
        source,
    }
}

/// What the start of an existing file says about its encoding: the encoding
/// to use and whether the file is empty.
fn detect_encoding(path: &Path, fallback: Encoding) -> io::Result<(Encoding, bool)> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut carry: Vec<u8> = Vec::new();
    let mut first = true;
    let mut valid = true;
    let mut empty = true;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or(&[]);
        if first {
            first = false;
            if chunk.starts_with(&BOM) {
                return Ok((Encoding::Utf8Bom, false));
            }
        }
        empty = false;
        if !valid {
            // Not UTF-8: the answer is Latin-1 whatever follows.
            break;
        }
        carry.extend_from_slice(chunk);
        match std::str::from_utf8(&carry) {
            Ok(_) => carry.clear(),
            Err(e) if e.error_len().is_none() => {
                // A character cut by the end of the chunk: keep its bytes.
                carry.drain(..e.valid_up_to());
            }
            Err(_) => valid = false,
        }
    }
    if empty {
        return Ok((fallback, true));
    }
    // A cut character left at the very end of the file is invalid.
    let valid = valid && carry.is_empty();
    Ok((
        if valid {
            Encoding::Utf8
        } else {
            Encoding::Latin1
        },
        false,
    ))
}

/// Encodes `text` into `out`; characters above U+00FF become `?` in Latin-1
/// and are counted.
fn encode(text: &str, encoding: Encoding, out: &mut Vec<u8>, unrepresentable: &mut u64) {
    match encoding {
        Encoding::Latin1 => out.extend(text.chars().map(|c| {
            u8::try_from(u32::from(c)).unwrap_or_else(|_| {
                *unrepresentable += 1;
                b'?'
            })
        })),
        _ => out.extend_from_slice(text.as_bytes()),
    }
}

fn newline_str(newline: Newline) -> &'static str {
    match newline {
        Newline::CrLf => "\r\n",
        _ => "\n",
    }
}

/// The sink of the games: a new file through a temporary, or the end of an
/// existing one.
struct Output {
    out: BufWriter<File>,
    /// The temporary file, renamed over `dest` by [`Output::finish`].
    temp: Option<PathBuf>,
    dest: PathBuf,
}

impl Output {
    fn write(&mut self, bytes: &[u8]) -> Result<(), ExportError> {
        let path = self.temp.as_deref().unwrap_or(&self.dest);
        self.out.write_all(bytes).map_err(io_error(path))
    }

    /// Flushes, syncs and renames; on any failure the temporary file is
    /// removed and the destination is left alone (D387).
    fn finish(self) -> Result<(), ExportError> {
        let temp = self.temp.clone();
        let result = self.finish_inner();
        if result.is_err()
            && let Some(temp) = temp
        {
            fs::remove_file(temp).ok();
        }
        result
    }

    fn finish_inner(mut self) -> Result<(), ExportError> {
        let path = self.temp.clone().unwrap_or_else(|| self.dest.clone());
        self.out.flush().map_err(io_error(&path))?;
        let file = self.out.into_inner().map_err(|e| ExportError::Io {
            path: path.clone(),
            source: e.into_error(),
        })?;
        file.sync_all().map_err(io_error(&path))?;
        // The Windows rename fails while the handle is open.
        drop(file);
        if let Some(temp) = &self.temp {
            fs::rename(temp, &self.dest).map_err(io_error(&self.dest))?;
        }
        Ok(())
    }

    fn abandon(self) {
        if let Some(temp) = self.temp {
            drop(self.out);
            fs::remove_file(temp).ok();
        }
    }
}

fn open_output(
    dest: &Path,
    options: &ExportOptions,
) -> Result<(Output, Encoding, bool), ExportError> {
    if options.append {
        let (encoding, empty) = detect_encoding(dest, options.encoding).map_err(io_error(dest))?;
        let file = OpenOptions::new()
            .append(true)
            .open(dest)
            .map_err(io_error(dest))?;
        let output = Output {
            out: BufWriter::with_capacity(OUTPUT_BUFFER, file),
            temp: None,
            dest: dest.to_owned(),
        };
        return Ok((output, encoding, empty));
    }
    let name = dest
        .file_name()
        .ok_or_else(|| ExportError::NoFileName {
            path: dest.to_owned(),
        })?
        .to_owned();
    // `create_new`: a file that is already there (a leftover or something of
    // the user's) is never truncated; the next free name is used instead.
    let mut attempt = 0u32;
    let (temp, file) = loop {
        let mut candidate = name.clone();
        if attempt > 0 {
            candidate.push(format!(".{attempt}"));
        }
        candidate.push(".tmp");
        let temp = dest.with_file_name(candidate);
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < 1000 => attempt += 1,
            Err(e) => return Err(io_error(&temp)(e)),
        }
    };
    let output = Output {
        out: BufWriter::with_capacity(OUTPUT_BUFFER, file),
        temp: Some(temp),
        dest: dest.to_owned(),
    };
    Ok((output, options.encoding, true))
}

/// Writes `games` to the PGN file `dest` (`.pgn` is added when the name does
/// not end in it, with any letter case; the report has the final path).
///
/// The progress is `(games handled, expected games)`, sent about every 100
/// games or 100 ms; the token is looked at after every game. Skipped games
/// do not stop the export.
///
/// A new file replaces `dest` only when the export ends (also when it is
/// cancelled, unless [`ExportOptions::discard_on_cancel`] is set); if an I/O error happens, the temporary file is removed and
/// `dest` is untouched. In append mode an error leaves what was already
/// written.
///
/// # Errors
///
/// [`ExportError::Io`] when a file cannot be opened, written or renamed
/// (append mode needs an existing file), [`ExportError::NoFileName`] for a
/// destination without a name.
pub fn export_pgn<I>(
    games: I,
    dest: &Path,
    options: &ExportOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ExportReport, ExportError>
where
    I: IntoIterator<Item = ExportItem>,
{
    let started = Instant::now();
    if dest.file_name().is_none() {
        return Err(ExportError::NoFileName {
            path: dest.to_owned(),
        });
    }
    let dest = with_pgn_extension(dest);
    let games = games.into_iter();
    let total = options.total.unwrap_or_else(|| {
        let (low, high) = games.size_hint();
        u64::try_from(high.unwrap_or(low)).unwrap_or(0)
    });
    let (mut output, encoding, fresh) = open_output(&dest, options)?;
    let result = write_all(
        games,
        &mut output,
        encoding,
        fresh,
        options,
        total,
        progress,
        cancel,
    );
    match result {
        Ok(mut report) => {
            let nothing = report.written == 0 && options.discard_if_empty;
            if (report.interrupted && options.discard_on_cancel || nothing) && output.temp.is_some()
            {
                output.abandon();
                report.path = dest;
                report.elapsed = started.elapsed();
                return Ok(report);
            }
            output.finish()?;
            report.path = dest;
            report.elapsed = started.elapsed();
            Ok(report)
        }
        Err(error) => {
            output.abandon();
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_all(
    games: impl Iterator<Item = ExportItem>,
    output: &mut Output,
    encoding: Encoding,
    fresh: bool,
    options: &ExportOptions,
    total: u64,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ExportReport, ExportError> {
    let mut report = ExportReport {
        path: PathBuf::new(),
        written: 0,
        skipped: Vec::new(),
        interrupted: false,
        unrepresentable: 0,
        elapsed: Duration::ZERO,
    };
    let mut pgn = options.pgn.clone();
    pgn.encoding = encoding;
    let newline = newline_str(pgn.newline);
    let mut buf: Vec<u8> = Vec::new();

    if fresh {
        if encoding == Encoding::Utf8Bom {
            output.write(&BOM)?;
        }
        // The start text belongs to new files only (`tkscid.cpp:1568`); an
        // empty file in append mode is a new file as far as the text goes.
        encode(
            &options.start_text,
            encoding,
            &mut buf,
            &mut report.unrepresentable,
        );
        output.write(&buf)?;
    }

    let mut ticker = Ticker::new(REPORT_EVERY, crate::progress::CHECK_INTERVAL);
    let mut handled = 0u64;
    let mut games = games;
    loop {
        if cancel.is_cancelled() {
            report.interrupted = true;
            break;
        }
        let Some(item) = games.next() else {
            break;
        };
        let (number, game) = match item {
            ExportItem::Game(game) => (0, Ok(game)),
            ExportItem::Stored { number, game } => (
                number,
                game.map_err(|e| ExportSkipReason::Unreadable(e.to_string())),
            ),
        };
        let rendered = game.and_then(|game| {
            game_to_string(&game, &pgn).map_err(|e| ExportSkipReason::Unwritable(e.to_string()))
        });
        match rendered {
            Ok(text) => {
                buf.clear();
                encode(&text, encoding, &mut buf, &mut report.unrepresentable);
                // A blank line follows every game.
                encode(newline, encoding, &mut buf, &mut report.unrepresentable);
                output.write(&buf)?;
                report.written += 1;
            }
            Err(reason) => report.skipped.push((number, reason)),
        }
        handled += 1;
        if ticker.tick() {
            progress.progress(handled, total.max(handled));
        }
    }
    buf.clear();
    encode(
        &options.end_text,
        encoding,
        &mut buf,
        &mut report.unrepresentable,
    );
    output.write(&buf)?;
    progress.progress(handled, total.max(handled));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::NoProgress;
    use crate::si4::Si4Database;
    use mychess_core::{GameResult, SanMode};
    use pretty_assertions::assert_eq;

    fn game(white: &str) -> Game {
        let mut game = Game::new();
        game.tags_mut().white = white.to_owned();
        game.tags_mut().result = GameResult::White;
        let root = game.root();
        let mv = game
            .position_at(root)
            .unwrap()
            .parse_san("e4", SanMode::Strict)
            .unwrap()
            .mv;
        game.add_move(root, mv).unwrap();
        game
    }

    fn items(names: &[&str]) -> Vec<ExportItem> {
        names
            .iter()
            .map(|n| ExportItem::Game(Box::new(game(n))))
            .collect()
    }

    fn export(
        items: Vec<ExportItem>,
        dest: &Path,
        options: &ExportOptions,
    ) -> Result<ExportReport, ExportError> {
        export_pgn(items, dest, options, &mut NoProgress, &CancelToken::new())
    }

    #[test]
    fn extension_is_added_without_looking_at_the_case() {
        assert_eq!(with_pgn_extension(Path::new("a")), Path::new("a.pgn"));
        assert_eq!(
            with_pgn_extension(Path::new("dir/")),
            Path::new("dir.pgn"),
            "a trailing separator is not part of the name"
        );
        assert_eq!(with_pgn_extension(Path::new("/")), Path::new("/"));
        assert_eq!(with_pgn_extension(Path::new("a.pgn")), Path::new("a.pgn"));
        assert_eq!(with_pgn_extension(Path::new("X.PGN")), Path::new("X.PGN"));
        assert_eq!(
            with_pgn_extension(Path::new("a.txt")),
            Path::new("a.txt.pgn")
        );
    }

    #[test]
    fn writes_games_with_a_blank_line_after_each() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        let options = ExportOptions {
            start_text: "% start\n".to_owned(),
            end_text: "% end\n".to_owned(),
            ..ExportOptions::scid_dialog()
        };
        let report = export(items(&["A", "B"]), &dest, &options).unwrap();
        assert_eq!(report.written, 2);
        assert_eq!(report.path, dir.path().join("out.pgn"));
        let text = fs::read_to_string(&report.path).unwrap();
        assert!(text.starts_with("% start\n[Event"), "{text}");
        assert!(text.ends_with("1. e4 1-0\n\n% end\n"), "{text}");
        assert_eq!(text.matches("[Event").count(), 2);
        assert!(!dir.path().join("out.pgn.tmp").exists());
    }

    #[test]
    fn upper_case_extension_is_not_doubled() {
        let dir = tempfile::tempdir().unwrap();
        let report = export(
            items(&["A"]),
            &dir.path().join("X.PGN"),
            &ExportOptions::default(),
        )
        .unwrap();
        assert_eq!(report.path, dir.path().join("X.PGN"));
        assert!(report.path.exists());
    }

    #[test]
    fn latin1_counts_what_does_not_fit() {
        let dir = tempfile::tempdir().unwrap();
        let options = ExportOptions {
            encoding: Encoding::Latin1,
            ..ExportOptions::default()
        };
        let report = export(
            items(&["Ren\u{e9} \u{20ac}\u{20ac}"]),
            &dir.path().join("l.pgn"),
            &options,
        )
        .unwrap();
        assert_eq!(report.unrepresentable, 2);
        let bytes = fs::read(&report.path).unwrap();
        let needle = b"[White \"Ren\xE9 ??\"]";
        assert!(bytes.windows(needle.len()).any(|w| w == needle));
    }

    #[test]
    fn utf8_bom_is_written_once() {
        let dir = tempfile::tempdir().unwrap();
        let options = ExportOptions {
            encoding: Encoding::Utf8Bom,
            ..ExportOptions::default()
        };
        let report = export(items(&["A", "B"]), &dir.path().join("b.pgn"), &options).unwrap();
        let bytes = fs::read(&report.path).unwrap();
        assert!(bytes.starts_with(&BOM));
        assert_eq!(bytes.windows(3).filter(|w| *w == BOM).count(), 1);
    }

    #[test]
    fn unreadable_games_are_skipped_and_listed() {
        let dir = tempfile::tempdir().unwrap();
        let mut list = items(&["A"]);
        list.insert(
            0,
            ExportItem::Stored {
                number: 7,
                game: Err(Si4Error::InvalidGameNumber { number: 7, len: 3 }),
            },
        );
        let report = export(list, &dir.path().join("s.pgn"), &ExportOptions::default()).unwrap();
        assert_eq!(report.written, 1);
        assert!(matches!(
            report.skipped.as_slice(),
            [(7, ExportSkipReason::Unreadable(_))]
        ));
    }

    #[test]
    fn cancel_keeps_the_games_written_and_the_end_text() {
        let dir = tempfile::tempdir().unwrap();
        let token = CancelToken::new();
        let seen = token.clone();
        let list = (0..5).map(move |i| {
            if i == 2 {
                seen.cancel();
            }
            ExportItem::Game(Box::new(game("A")))
        });
        let options = ExportOptions {
            end_text: "% end\n".to_owned(),
            ..ExportOptions::default()
        };
        let report = export_pgn(
            list,
            &dir.path().join("c.pgn"),
            &options,
            &mut NoProgress,
            &token,
        )
        .unwrap();
        assert!(report.interrupted);
        assert_eq!(report.written, 3);
        assert!(
            fs::read_to_string(&report.path)
                .unwrap()
                .ends_with("% end\n")
        );
        assert!(!dir.path().join("c.pgn.tmp").exists());
    }

    #[test]
    fn cancel_can_discard_the_new_file_and_keep_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("d.pgn");
        fs::write(&dest, "old").unwrap();
        let token = CancelToken::new();
        let seen = token.clone();
        let list = (0..5).map(move |i| {
            if i == 2 {
                seen.cancel();
            }
            ExportItem::Game(Box::new(game("A")))
        });
        let options = ExportOptions {
            discard_on_cancel: true,
            ..ExportOptions::default()
        };
        let report = export_pgn(list, &dest, &options, &mut NoProgress, &token).unwrap();
        assert!(report.interrupted);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "old");
        assert!(!dir.path().join("d.pgn.tmp").exists());
    }

    #[test]
    fn a_file_named_like_the_temporary_is_never_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("t.pgn");
        fs::write(dir.path().join("t.pgn.tmp"), "mine").unwrap();
        let report = export(items(&["A"]), &dest, &ExportOptions::default()).unwrap();
        assert_eq!(report.written, 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("t.pgn.tmp")).unwrap(),
            "mine"
        );
        assert!(!dir.path().join("t.pgn.1.tmp").exists());
    }

    #[test]
    fn nothing_written_can_leave_the_old_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("e.pgn");
        fs::write(&dest, "old").unwrap();
        let bad = || {
            vec![ExportItem::Stored {
                number: 9,
                game: Err(Si4Error::InvalidGameNumber { number: 9, len: 0 }),
            }]
        };
        let options = ExportOptions {
            discard_if_empty: true,
            ..ExportOptions::default()
        };
        let report = export(bad(), &dest, &options).unwrap();
        assert_eq!((report.written, report.skipped.len()), (0, 1));
        assert_eq!(fs::read_to_string(&dest).unwrap(), "old");
        assert!(!dir.path().join("e.pgn.tmp").exists());
        // The default still writes the (empty) file.
        export(bad(), &dest, &ExportOptions::default()).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "");
    }

    #[test]
    fn append_follows_the_encoding_of_the_file_and_skips_the_start_text() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.pgn");
        let mut first = BOM.to_vec();
        first.extend_from_slice("% old\n".as_bytes());
        fs::write(&dest, &first).unwrap();
        let options = ExportOptions {
            append: true,
            start_text: "% start\n".to_owned(),
            end_text: "% end\n".to_owned(),
            encoding: Encoding::Latin1,
            ..ExportOptions::default()
        };
        export(items(&["\u{e9}"]), &dest, &options).unwrap();
        let bytes = fs::read(&dest).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("\u{feff}% old\n[Event"), "{text}");
        assert!(text.contains("[White \"\u{e9}\"]"));
        assert!(!text.contains("% start"));
        assert!(text.ends_with("% end\n"));
        assert_eq!(text.matches('\u{feff}').count(), 1);
    }

    #[test]
    fn append_to_a_latin1_file_writes_latin1() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.pgn");
        fs::write(&dest, b"% caf\xE9\n").unwrap();
        let options = ExportOptions {
            append: true,
            ..ExportOptions::default()
        };
        export(items(&["\u{e9}"]), &dest, &options).unwrap();
        let bytes = fs::read(&dest).unwrap();
        let needle = b"[White \"\xE9\"]";
        assert!(bytes.windows(needle.len()).any(|w| w == needle));
    }

    #[test]
    fn append_to_a_missing_file_fails() {
        let dir = tempfile::tempdir().unwrap();
        let options = ExportOptions {
            append: true,
            ..ExportOptions::default()
        };
        let result = export(items(&["A"]), &dir.path().join("none.pgn"), &options);
        assert!(matches!(result, Err(ExportError::Io { .. })));
    }

    #[test]
    fn utf8_split_across_read_chunks_is_still_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("big.pgn");
        let mut data = "a".repeat(64 * 1024 - 1).into_bytes();
        data.extend_from_slice("\u{e9}tude".as_bytes());
        fs::write(&dest, &data).unwrap();
        assert_eq!(
            detect_encoding(&dest, Encoding::Latin1).unwrap(),
            (Encoding::Utf8, false)
        );
    }

    #[test]
    fn empty_destination_directory_name_is_an_error_not_a_panic() {
        let result = export(items(&["A"]), Path::new(""), &ExportOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn round_trip_through_a_database() {
        use crate::si4::Si4Writer;
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&dir.path().join("db"), 0).unwrap();
        writer
            .add_game(&game("A"), crate::si4::GameFlags::empty())
            .unwrap();
        writer
            .add_game(&game("B"), crate::si4::GameFlags::empty())
            .unwrap();
        drop(writer);
        let mut db = Si4Database::open(&dir.path().join("db")).unwrap();
        let list: Vec<ExportItem> = (1..=db.len())
            .map(|n| ExportItem::stored(&mut db, n))
            .collect();
        let report = export(list, &dir.path().join("o.pgn"), &ExportOptions::default()).unwrap();
        assert_eq!(report.written, 2);
        assert_eq!(report.skipped.len(), 0);
        assert!(
            fs::read_to_string(&report.path)
                .unwrap()
                .contains("[White \"B\"]")
        );
    }
}
