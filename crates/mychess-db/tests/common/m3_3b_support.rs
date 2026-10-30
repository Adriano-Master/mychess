//! Helpers of the M3-3b contract tests (PGN import and export).
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc
)]

use std::fmt::Write as _;
use std::fs::File;
use std::path::PathBuf;

use mychess_core::Game;
use mychess_db::si4::{Si4Database, Si4Writer};
use mychess_db::{CancelToken, ImportOptions, ImportReport, NoProgress, import_pgn};
use mychess_pgn::write::{WriteOptions, game_to_string};

pub const THREE: &str = "[Event \"One\"]\n[Site \"Paris\"]\n[White \"Alpha\"]\n[Black \"Beta\"]\n\
    [Result \"1-0\"]\n[ECO \"C44\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
    [Event \"Two\"]\n[Site \"Rome\"]\n[White \"Gamma\"]\n[Black \"Delta\"]\n\
    [Result \"0-1\"]\n[ECO \"D06\"]\n\n1. d4 d5 2. c4 0-1\n\n\
    [Event \"Three\"]\n[Site \"Oslo\"]\n[White \"Alpha\"]\n[Black \"Delta\"]\n\
    [Result \"1/2-1/2\"]\n\n1. c4 c5 1/2-1/2\n";

/// A database made by importing a PGN text into a new directory.
pub struct Imported {
    pub dir: tempfile::TempDir,
    pub base: PathBuf,
    pub report: ImportReport,
}

impl Imported {
    pub fn open(&self) -> Si4Database<File> {
        Si4Database::open(&self.base).unwrap()
    }
}

pub fn import_bytes(pgn: &[u8], options: &ImportOptions) -> Imported {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let report = import_pgn(
        pgn,
        &mut writer,
        options,
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    drop(writer);
    Imported { dir, base, report }
}

pub fn import_str(pgn: &str, options: &ImportOptions) -> Imported {
    import_bytes(pgn.as_bytes(), options)
}

/// `n` small games with distinct events.
pub fn many_games(n: usize) -> String {
    let mut pgn = String::new();
    for i in 0..n {
        write!(pgn, "[Event \"E{i}\"]\n[Result \"*\"]\n\n1. e4 e5 *\n\n").unwrap();
    }
    pgn
}

/// Canonical PGN text of a game (strict writer), the oracle for "same game".
pub fn canon(game: &Game) -> String {
    game_to_string(game, &WriteOptions::default()).unwrap()
}

/// Names of the files of a directory, sorted.
pub fn file_names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// CRC-32 (IEEE), bitwise; enough for small test inputs.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// One gzip member holding `data` in stored (uncompressed) deflate blocks, so
/// that the tests need no compression crate.
pub fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF];
    let mut chunks = data.chunks(65_535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(chunk) = chunks.next() {
        let last = u8::from(chunks.peek().is_none());
        let len = u16::try_from(chunk.len()).unwrap();
        out.push(last);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out
}

/// Points the record of game `n` far beyond the end of the `.sg4`, so that
/// decoding it fails while the index still opens.
pub fn break_record(base: &std::path::Path, n: u32) {
    const HEADER_LEN: usize = 182;
    const ENTRY_LEN: usize = 47;
    let path = base.with_extension("si4");
    let mut bytes = std::fs::read(&path).unwrap();
    let at = HEADER_LEN + ENTRY_LEN * (n as usize - 1);
    bytes[at..at + 4].copy_from_slice(&0x7FFF_FF00_u32.to_be_bytes());
    std::fs::write(&path, bytes).unwrap();
}
