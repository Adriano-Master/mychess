//! Reading PGN text and files into games (spec M2-2, D119-D124).
//!
//! Blocking and CPU-bound: only [`perform`](crate::perform) calls it, from
//! `spawn_blocking`.

use std::cell::Cell;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flate2::read::MultiGzDecoder;

use mychess_core::Position;
use mychess_pgn::read::{
    DiagnosticKind, GameChunker, ParsedGame, RawGame, ReadOptions, Severity, parse_movetext,
};

use super::collection::PgnCollection;
use crate::error::{FileError, FileErrorKind};
use crate::import::{ImportIssue, ImportMode, ImportNotice, ImportReport, ScanLimit};
use crate::shared::SharedGame;

/// Limits of a scan (D124): reading stops there and a
/// [`ImportIssue::Truncated`] notice is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// Most decompressed bytes read.
    pub max_bytes: u64,
    /// Most games kept.
    pub max_games: usize,
}

impl Default for ScanLimits {
    /// 64 MiB and 50 000 games.
    fn default() -> ScanLimits {
        ScanLimits {
            max_bytes: 64 << 20,
            max_games: 50_000,
        }
    }
}

/// Where the PGN comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PgnSource {
    /// Pasted text.
    Text(String),
    /// A file (plain or gzip, detected by content).
    File(PathBuf),
}

/// One game read from PGN, with its report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedGame {
    /// The game.
    pub game: SharedGame,
    /// Number of the game in its file (1-based).
    pub number: u64,
    /// Everything worth telling about the reading.
    pub report: ImportReport,
}

/// What a scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PgnLoad {
    /// Exactly one game (or bare moves).
    Single(LoadedGame),
    /// Several games, not parsed yet.
    Many(Arc<PgnCollection>),
    /// Nothing usable (for text only; a file with no game is an error).
    None {
        /// Bytes of text outside any game.
        skipped_bytes: u64,
    },
}

/// The result of a scan: what was found and collection-level remarks
/// (truncation, text outside games).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOutcome {
    /// What was found.
    pub load: PgnLoad,
    /// Remarks that belong to the whole input.
    pub notices: Vec<ImportNotice>,
    /// The canonical path of the file, `None` for text.
    pub path: Option<PathBuf>,
}

struct Collected {
    raws: Vec<RawGame>,
    skipped: u64,
    encoding: &'static str,
    truncated: Option<(u64, ScanLimit)>,
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Slack above `max_bytes + max_game_bytes` that the reader allows: the
/// chunker reads ahead in blocks of 1 MiB.
const READ_AHEAD_SLACK: u64 = 2 << 20;

/// What the input layers saw, shared with [`collect`].
#[derive(Default)]
struct ReadState {
    /// The reader refused to go past the cap.
    exceeded: Cell<bool>,
    /// The input is a gzip stream (decompressed by us).
    gzip: Cell<bool>,
    /// The file itself (not the decompressor) failed to read.
    source_failed: Cell<bool>,
}

/// Passes on at most `remaining` bytes; if the source has more, fails and
/// sets [`ReadState::exceeded`]. Applied to the **decompressed** stream, so
/// neither a gzip bomb nor a file without any game header can keep the
/// thread reading forever.
struct LimitedReader<R> {
    inner: R,
    remaining: u64,
    state: Rc<ReadState>,
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            // At the cap: fine only if the source ends here.
            let mut probe = [0u8; 1];
            if self.inner.read(&mut probe)? == 0 {
                return Ok(0);
            }
            self.state.exceeded.set(true);
            return Err(io::Error::other("input exceeds the size limit"));
        }
        let room = usize::try_from(self.remaining).map_or(out.len(), |r| r.min(out.len()));
        let n = self.inner.read(&mut out[..room])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

/// Records that the underlying file (not the gzip layer) failed.
struct TrackedFile {
    inner: BufReader<File>,
    state: Rc<ReadState>,
}

impl Read for TrackedFile {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.inner
            .read(out)
            .inspect_err(|_| self.state.source_failed.set(true))
    }
}

impl BufRead for TrackedFile {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self.inner.fill_buf() {
            Ok(buf) => Ok(buf),
            Err(e) => {
                self.state.source_failed.set(true);
                Err(e)
            }
        }
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

fn byte_cap(limits: &ScanLimits, opts: &ReadOptions) -> u64 {
    limits
        .max_bytes
        .saturating_add(count(opts.max_game_bytes))
        .saturating_add(READ_AHEAD_SLACK)
}

fn collect<R: Read>(
    input: R,
    path: Option<&Path>,
    limits: &ScanLimits,
    opts: &ReadOptions,
    state: &Rc<ReadState>,
) -> Result<Collected, FileError> {
    let input = LimitedReader {
        inner: input,
        remaining: byte_cap(limits, opts),
        state: Rc::clone(state),
    };
    let mut chunker = GameChunker::new(input, opts.max_game_bytes);
    let mut raws = Vec::new();
    let mut truncated = None;
    while let Some(item) = chunker.next() {
        if state.exceeded.get() {
            // The reader refused to go on. What the chunker hands out now
            // is the tail cut at the cap (or the error that follows it):
            // not a game to keep.
            if raws.is_empty() {
                return Err(FileError::new(
                    FileErrorKind::TooLarge {
                        limit: limits.max_bytes,
                    },
                    path.map(Path::to_path_buf),
                ));
            }
            truncated = Some((count(raws.len()), ScanLimit::Bytes(limits.max_bytes)));
            break;
        }
        let raw = match item {
            Ok(raw) => raw,
            Err(e) if state.gzip.get() && !state.source_failed.get() => {
                // The decompressor rejected its data.
                drop(e);
                return Err(FileError::new(
                    FileErrorKind::CorruptGzip,
                    path.map(Path::to_path_buf),
                ));
            }
            Err(e) => return Err(FileError::from_pgn(&e, path.map(Path::to_path_buf))),
        };
        if raws.len() >= limits.max_games {
            truncated = Some((count(raws.len()), ScanLimit::Games(limits.max_games)));
            break;
        }
        if chunker.bytes_consumed() > limits.max_bytes {
            truncated = Some((count(raws.len()), ScanLimit::Bytes(limits.max_bytes)));
            break;
        }
        raws.push(raw);
    }
    Ok(Collected {
        raws,
        skipped: chunker.skipped_bytes(),
        encoding: chunker.fallback_encoding().name(),
        truncated,
    })
}

/// Opens `path` and decompresses it when it holds gzip data (by content,
/// not by name).
fn open_decoded(path: &Path, state: &Rc<ReadState>) -> Result<Box<dyn Read>, FileError> {
    let fail = |e: &io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    let file = File::open(path).map_err(|e| fail(&e))?;
    let mut tracked = TrackedFile {
        inner: BufReader::new(file),
        state: Rc::clone(state),
    };
    let head = tracked.fill_buf().map_err(|e| fail(&e))?;
    if !head.starts_with(&[0x1f, 0x8b]) {
        return Ok(Box::new(tracked));
    }
    state.gzip.set(true);
    let mut decoded = BufReader::new(MultiGzDecoder::new(tracked));
    // A gzip stream inside a gzip stream would escape the size cap.
    match decoded.fill_buf() {
        Ok([0x1f, 0x8b, ..]) => Err(FileError::new(
            FileErrorKind::UnsupportedEncoding("nested gzip"),
            Some(path.to_path_buf()),
        )),
        Ok(_) => Ok(Box::new(decoded)),
        Err(e) if state.source_failed.get() => Err(fail(&e)),
        Err(_) => Err(FileError::new(
            FileErrorKind::CorruptGzip,
            Some(path.to_path_buf()),
        )),
    }
}

fn loaded_from_parsed(parsed: ParsedGame) -> LoadedGame {
    LoadedGame {
        number: parsed.number,
        report: ImportReport {
            mode: ImportMode::FullGame,
            notices: parsed
                .diagnostics
                .iter()
                .map(ImportNotice::from_diagnostic)
                .collect(),
        },
        game: SharedGame::new(parsed.game),
    }
}

fn collection_notices(skipped: u64, truncated: Option<(u64, ScanLimit)>) -> Vec<ImportNotice> {
    let mut notices = Vec::new();
    if skipped > 0 {
        notices.push(ImportNotice::new(
            Severity::Info,
            ImportIssue::TextOutsideGames { bytes: skipped },
        ));
    }
    if let Some((games_read, limit)) = truncated {
        notices.push(ImportNotice::new(
            Severity::Warning,
            ImportIssue::Truncated { games_read, limit },
        ));
    }
    notices
}

/// Reads PGN from text or from a file.
///
/// - One game: [`PgnLoad::Single`]. Several: [`PgnLoad::Many`] (kept raw,
///   parsed on demand).
/// - Text without any header line is read as bare moves from the standard
///   position, with no "missing result" remark (D120).
/// - A file with no game is [`FileErrorKind::NoGamesFound`]; a corrupt gzip
///   stream or UTF-16 is an error and no game is offered.
/// - Reading stops at `limits`, with a `Truncated` notice. The byte limit
///   also caps the **decompressed** stream (plus one game and the read-ahead
///   block): input that never yields a game within that cap, such as a gzip
///   bomb or a huge file without headers, fails with
///   [`FileErrorKind::TooLarge`] instead of holding the thread.
///
/// # Errors
///
/// A [`FileError`] for I/O failures, corrupt gzip, unsupported encoding or
/// a file without games.
pub fn scan_pgn(
    source: PgnSource,
    limits: &ScanLimits,
    opts: &ReadOptions,
) -> Result<(PgnLoad, Vec<ImportNotice>), FileError> {
    match source {
        PgnSource::Text(text) => scan_text(&text, limits, opts),
        PgnSource::File(path) => scan_file(&path, limits, opts),
    }
}

fn scan_file(
    path: &Path,
    limits: &ScanLimits,
    opts: &ReadOptions,
) -> Result<(PgnLoad, Vec<ImportNotice>), FileError> {
    let state = Rc::new(ReadState::default());
    let input = open_decoded(path, &state)?;
    let found = collect(input, Some(path), limits, opts, &state)?;
    let notices = collection_notices(found.skipped, found.truncated);
    let load = match found.raws.len() {
        0 => {
            return Err(FileError::new(
                FileErrorKind::NoGamesFound {
                    skipped_bytes: found.skipped,
                },
                Some(path.to_path_buf()),
            ));
        }
        1 => match found.raws.first() {
            Some(raw) => PgnLoad::Single(loaded_from_parsed(raw.parse(opts))),
            None => PgnLoad::None {
                skipped_bytes: found.skipped,
            },
        },
        _ => PgnLoad::Many(Arc::new(PgnCollection::new(
            found.raws,
            Some(path.to_path_buf()),
            found.encoding,
            found.skipped,
            found.truncated,
        ))),
    };
    Ok((load, notices))
}

fn scan_text(
    text: &str,
    limits: &ScanLimits,
    opts: &ReadOptions,
) -> Result<(PgnLoad, Vec<ImportNotice>), FileError> {
    let state = Rc::new(ReadState::default());
    let found = collect(text.as_bytes(), None, limits, opts, &state)?;
    let mut notices = collection_notices(found.skipped, found.truncated);
    let load = match found.raws.len() {
        0 => {
            let (load, extra) = moves_only(text, found.skipped, opts);
            notices = extra;
            load
        }
        1 => match found.raws.first() {
            Some(raw) => PgnLoad::Single(loaded_from_parsed(raw.parse(opts))),
            None => PgnLoad::None {
                skipped_bytes: found.skipped,
            },
        },
        _ => PgnLoad::Many(Arc::new(PgnCollection::new(
            found.raws,
            None,
            found.encoding,
            found.skipped,
            found.truncated,
        ))),
    };
    Ok((load, notices))
}

/// The text has no header: read it as moves from the standard position.
fn moves_only(text: &str, skipped: u64, opts: &ReadOptions) -> (PgnLoad, Vec<ImportNotice>) {
    let parsed = parse_movetext(text, &Position::new(), opts);
    // The Scid turns these two warnings off for bare moves.
    let mut notices: Vec<ImportNotice> = parsed
        .diagnostics
        .iter()
        .filter(|d| {
            !matches!(
                d.kind,
                DiagnosticKind::MissingResult | DiagnosticKind::HeaderOnlyAtEof
            )
        })
        .map(ImportNotice::from_diagnostic)
        .collect();
    let has_content = parsed.game.mainline().next().is_some() || parsed.result.is_some();
    if !has_content {
        notices.push(ImportNotice::new(
            Severity::Error,
            ImportIssue::NothingToImport,
        ));
        return (
            PgnLoad::None {
                skipped_bytes: skipped,
            },
            notices,
        );
    }
    let game = LoadedGame {
        game: SharedGame::new(parsed.game),
        number: 1,
        report: ImportReport {
            mode: ImportMode::MovesOnly,
            notices,
        },
    };
    (PgnLoad::Single(game), Vec::new())
}

#[cfg(test)]
mod tests {
    use mychess_core::GameResult;

    use super::*;

    fn text(t: &str) -> Result<(PgnLoad, Vec<ImportNotice>), FileError> {
        scan_pgn(
            PgnSource::Text(t.to_owned()),
            &ScanLimits::default(),
            &ReadOptions::default(),
        )
    }

    fn single(t: &str) -> LoadedGame {
        match text(t) {
            Ok((PgnLoad::Single(game), _)) => game,
            other => panic!("expected a single game, got {other:?}"),
        }
    }

    #[test]
    fn bare_moves_load_without_result_warning() {
        let loaded = single("1. e4 e5 2. Nf3");
        assert_eq!(loaded.report.mode, ImportMode::MovesOnly);
        assert_eq!(loaded.game.mainline().count(), 3);
        assert!(loaded.report.notices.is_empty(), "{:?}", loaded.report);
    }

    #[test]
    fn a_complete_game_loads_clean() {
        let loaded = single("[White \"A\"]\n\n1. e4 e5 1-0");
        assert_eq!(loaded.report.mode, ImportMode::FullGame);
        assert_eq!(loaded.game.tags().result, GameResult::White);
        assert!(loaded.report.notices.is_empty(), "{:?}", loaded.report);
    }

    #[test]
    fn illegal_moves_are_reported_with_a_line() {
        let loaded = single("[Event \"x\"]\n\n1. e4 e5 2. Ke4 *");
        assert_eq!(loaded.game.mainline().count(), 2);
        let notice = loaded
            .report
            .notices
            .iter()
            .find(|n| matches!(n.issue, ImportIssue::Pgn(DiagnosticKind::IllegalMove(_))))
            .expect("an illegal move notice");
        assert_eq!(notice.severity, Severity::Error);
        assert_eq!(notice.line, Some(3));
        assert!(loaded.report.has_problems());
    }

    #[test]
    fn several_games_make_a_collection() {
        let pgn = "[White \"A\"]\n\n1. e4 *\n\n[White \"B\"]\n\n1. d4 *\n";
        let Ok((PgnLoad::Many(collection), _)) = text(pgn) else {
            panic!("expected a collection");
        };
        assert_eq!(collection.len(), 2);
        assert_eq!(collection.header(1).map(|h| h.white.as_str()), Some("B"));
        assert_eq!(collection.filter("b"), vec![1]);
        assert_eq!(collection.filter(""), vec![0, 1]);
    }

    #[test]
    fn blank_and_junk_text_import_nothing() {
        for t in ["", "   \n\t", "hello world"] {
            let Ok((PgnLoad::None { .. }, notices)) = text(t) else {
                panic!("expected nothing for {t:?}");
            };
            assert!(
                notices
                    .iter()
                    .any(|n| n.issue == ImportIssue::NothingToImport),
                "{t:?}"
            );
        }
    }

    #[test]
    fn limits_truncate_with_a_notice() {
        let one = "[Event \"x\"]\n\n1. e4 *\n\n";
        let pgn = one.repeat(5);
        let limits = ScanLimits {
            max_games: 3,
            ..ScanLimits::default()
        };
        let (load, notices) = scan_pgn(
            PgnSource::Text(pgn.clone()),
            &limits,
            &ReadOptions::default(),
        )
        .expect("scan");
        let PgnLoad::Many(collection) = load else {
            panic!("expected a collection");
        };
        assert_eq!(collection.len(), 3);
        assert!(notices.iter().any(|n| matches!(
            n.issue,
            ImportIssue::Truncated {
                games_read: 3,
                limit: ScanLimit::Games(3)
            }
        )));
        let limits = ScanLimits {
            max_bytes: 40,
            ..ScanLimits::default()
        };
        let (load, notices) =
            scan_pgn(PgnSource::Text(pgn), &limits, &ReadOptions::default()).expect("scan");
        assert!(notices.iter().any(|n| matches!(
            n.issue,
            ImportIssue::Truncated {
                limit: ScanLimit::Bytes(40),
                ..
            }
        )));
        match load {
            PgnLoad::Many(c) => assert!(c.len() < 5),
            PgnLoad::Single(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn files_are_read_and_errors_are_structured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("one game.pgn");
        std::fs::write(&path, "[White \"\u{e9}\"]\n\n1. e4 *\n").expect("write");
        let (load, _) = scan_pgn(
            PgnSource::File(path.clone()),
            &ScanLimits::default(),
            &ReadOptions::default(),
        )
        .expect("scan");
        let PgnLoad::Single(loaded) = load else {
            panic!("expected one game");
        };
        assert_eq!(loaded.game.tags().white, "\u{e9}");

        let empty = dir.path().join("empty.pgn");
        std::fs::write(&empty, "just words, no header\n").expect("write");
        let err = scan_pgn(
            PgnSource::File(empty),
            &ScanLimits::default(),
            &ReadOptions::default(),
        )
        .expect_err("no games");
        assert!(
            matches!(err.kind, FileErrorKind::NoGamesFound { skipped_bytes } if skipped_bytes > 0)
        );

        let err = scan_pgn(
            PgnSource::File(dir.path().join("missing.pgn")),
            &ScanLimits::default(),
            &ReadOptions::default(),
        )
        .expect_err("missing");
        assert_eq!(err.kind, FileErrorKind::NotFound);

        let utf16 = dir.path().join("utf16.pgn");
        std::fs::write(&utf16, [0xff, 0xfe, b'[', 0]).expect("write");
        let err = scan_pgn(
            PgnSource::File(utf16),
            &ScanLimits::default(),
            &ReadOptions::default(),
        )
        .expect_err("utf16");
        assert!(matches!(err.kind, FileErrorKind::UnsupportedEncoding(_)));
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(data).expect("compress");
        enc.finish().expect("finish")
    }

    fn scan_file_with(
        bytes: &[u8],
        limits: &ScanLimits,
    ) -> Result<(PgnLoad, Vec<ImportNotice>), FileError> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("in.bin");
        std::fs::write(&path, bytes).expect("write");
        let opts = ReadOptions::default().with_max_game_bytes(1000);
        scan_pgn(PgnSource::File(path), limits, &opts)
    }

    fn small_limits() -> ScanLimits {
        ScanLimits {
            max_bytes: 1000,
            max_games: 10,
        }
    }

    #[test]
    fn a_gzip_bomb_is_refused_at_the_cap() {
        // 8 MiB of text without any header compresses to a few KiB.
        let junk = b"x".repeat(8 << 20);
        let bomb = gzip(&junk);
        assert!(bomb.len() < 100_000, "{}", bomb.len());
        let err = scan_file_with(&bomb, &small_limits()).expect_err("too large");
        assert_eq!(err.kind, FileErrorKind::TooLarge { limit: 1000 });
    }

    #[test]
    fn a_huge_file_without_headers_is_refused_at_the_cap() {
        let junk = b"just words, no header\n".repeat(300_000);
        assert!(junk.len() > 5 << 20);
        let err = scan_file_with(&junk, &small_limits()).expect_err("too large");
        assert_eq!(err.kind, FileErrorKind::TooLarge { limit: 1000 });
    }

    #[test]
    fn a_gzip_file_within_the_limits_is_read() {
        let pgn = "[Event \"x\"]\n\n1. e4 e5 *\n";
        let (load, notices) = scan_file_with(&gzip(pgn.as_bytes()), &small_limits()).expect("scan");
        assert!(matches!(load, PgnLoad::Single(_)));
        assert!(notices.is_empty());
    }

    #[test]
    fn corrupt_and_nested_gzip_are_structured_errors() {
        let good = gzip(&b"[Event \"x\"]\n\n1. e4 *\n".repeat(2000));
        let cut = &good[..good.len() / 2];
        let err = scan_file_with(cut, &ScanLimits::default()).expect_err("truncated");
        assert_eq!(err.kind, FileErrorKind::CorruptGzip);
        let mut garbage = vec![0x1f, 0x8b, 8, 0];
        garbage.extend_from_slice(&[0xff; 64]);
        let err = scan_file_with(&garbage, &ScanLimits::default()).expect_err("garbage");
        assert_eq!(err.kind, FileErrorKind::CorruptGzip);
        let nested = gzip(&gzip(b"[Event \"x\"]\n\n1. e4 *\n"));
        let err = scan_file_with(&nested, &ScanLimits::default()).expect_err("nested");
        assert!(matches!(err.kind, FileErrorKind::UnsupportedEncoding(_)));
    }

    #[test]
    fn exactly_the_cap_is_not_an_error() {
        let mut reader = LimitedReader {
            inner: &b"abcd"[..],
            remaining: 4,
            state: Rc::new(ReadState::default()),
        };
        let mut out = Vec::new();
        reader.read_to_end(&mut out).expect("at the cap");
        assert_eq!(out, b"abcd");
        let mut reader = LimitedReader {
            inner: &b"abcde"[..],
            remaining: 4,
            state: Rc::new(ReadState::default()),
        };
        assert!(reader.read_to_end(&mut Vec::new()).is_err());
        assert!(reader.state.exceeded.get());
    }

    #[test]
    fn games_before_a_runaway_tail_are_kept_with_a_truncation_notice() {
        let one = "[Event \"x\"]\n\n1. e4 e5 *\n\n";
        let mut bytes = one.repeat(3).into_bytes();
        bytes.extend_from_slice(&b"just words, no header\n".repeat(300_000));
        let (load, notices) = scan_file_with(&bytes, &small_limits()).expect("scan");
        let PgnLoad::Many(collection) = load else {
            panic!("expected the games before the tail");
        };
        assert!(collection.len() >= 2);
        assert!(
            notices
                .iter()
                .any(|n| matches!(n.issue, ImportIssue::Truncated { .. })),
            "{notices:?}"
        );
    }
}
