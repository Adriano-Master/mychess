//! Helpers of the M5-3a contract tests (tree cache file `.stc`). The byte
//! builders below follow the layout of D1043 of `docs/specs/m5-3-cache-stc.md`
//! and know nothing about the production encoder.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::cast_possible_truncation
)]

use std::path::{Path, PathBuf};

use mychess_core::{Position, Square};
use mychess_db::Filter;
use mychess_db::si4::RawMove;
use mychess_db::tree::stc::StcCache;
use mychess_db::tree::{CachedTree, TreeCache, TreeNode};

/// The magic and version (400 = 0x0190) of every file, then the four u32.
pub fn header(cache_size: u32, in_use: u32, lowest_total: u32, lowest_index: u32) -> Vec<u8> {
    let mut v = vec![0x73, 0x74, 0x63, 0x18, 0x01, 0x90];
    for n in [cache_size, in_use, lowest_total, lowest_index] {
        v.extend_from_slice(&n.to_be_bytes());
    }
    assert_eq!(v.len(), 22);
    v
}

/// The board of the standard start position in the file encoding
/// (A1 = index 0; empty 7; white K1 Q2 R3 B4 N5 P6; black +8).
pub fn start_board() -> [u8; 64] {
    let mut b = [7u8; 64];
    let back = [3u8, 5, 4, 2, 1, 4, 5, 3];
    for (file, piece) in back.iter().enumerate() {
        b[file] = *piece;
        b[56 + file] = piece + 8;
        b[8 + file] = 6;
        b[48 + file] = 14;
    }
    b
}

/// One node of 71 bytes: `from`/`to`/`promote` as file codes, results in
/// the FILE order `[no result, white, black, draw]`.
pub fn node(from: u8, to: u8, promote: u8, file_freq: [u32; 4], total: u32) -> Vec<u8> {
    let mut v = vec![
        0, 0, from, to, 0, 7, promote, 0, // piece nums, squares, reserved
        0, 0, 65, // capturedSquare, castleFlags, epSquare
        0, 0, // oldHalfMoveClock
    ];
    v.extend_from_slice(&[0u8; 8]); // san
    for f in file_freq {
        v.extend_from_slice(&f.to_be_bytes());
    }
    v.extend_from_slice(&total.to_be_bytes());
    v.extend_from_slice(&0u32.to_be_bytes()); // score
    v.extend_from_slice(&0u16.to_be_bytes()); // ecoCode
    for n in [0u32; 6] {
        v.extend_from_slice(&n.to_be_bytes());
    }
    assert_eq!(v.len(), 71);
    v
}

/// A node with the Elo / performance / year fields.
pub fn node_full(from: u8, to: u8, file_freq: [u32; 4], total: u32, stats: [u32; 6]) -> Vec<u8> {
    let mut v = node(from, to, 7, file_freq, total);
    let at = 71 - 24;
    for (i, n) in stats.iter().enumerate() {
        v[at + i * 4..at + i * 4 + 4].copy_from_slice(&n.to_be_bytes());
    }
    v
}

/// An entry: side (0/1), board, `moveCount` (as given, not derived), total,
/// the node bytes, then the compressed filter block.
pub fn entry(
    to_move: u8,
    board: &[u8; 64],
    move_count: u32,
    total: u32,
    nodes: &[Vec<u8>],
    filter: (u32, u32, &[u8]),
) -> Vec<u8> {
    let mut v = vec![to_move];
    v.extend_from_slice(board);
    v.extend_from_slice(&move_count.to_be_bytes());
    v.extend_from_slice(&total.to_be_bytes());
    for n in nodes {
        v.extend_from_slice(n);
    }
    v.extend_from_slice(&filter.0.to_be_bytes());
    v.extend_from_slice(&filter.1.to_be_bytes());
    v.extend_from_slice(&(filter.2.len() as u32).to_be_bytes());
    v.extend_from_slice(filter.2);
    v
}

/// A coherent entry of the start position with one node (e2e4: 12 -> 28)
/// and a filter of `len` games where game 1 is in (copy form).
pub fn simple_entry(len: u32, total: u32) -> Vec<u8> {
    let mut data = vec![0u8]; // flag: copied
    for i in 0..len {
        data.push(u8::from(i == 0) * 2);
    }
    entry(
        0,
        &start_board(),
        1,
        total,
        &[node(12, 28, 7, [0, total, 0, 0], total)],
        (len, u32::from(len > 0), &data),
    )
}

pub fn file(header: Vec<u8>, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut v = header;
    for e in entries {
        v.extend_from_slice(e);
    }
    v
}

/// A fresh temporary directory and the base path `games` inside it.
pub fn tmp_base() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("games");
    (dir, base)
}

pub fn stc_file(base: &Path) -> PathBuf {
    base.with_extension("stc")
}

pub fn write_stc(base: &Path, bytes: &[u8]) {
    std::fs::write(stc_file(base), bytes).unwrap();
}

/// The nodes of a `CachedTree` for the tests: one node `e2e4` with
/// `total` games, all of them White wins.
pub fn tree_of(len: u32, total: u32) -> CachedTree {
    let mut filter = Filter::new(len);
    filter.fill(0);
    filter.set(1, 2);
    CachedTree {
        nodes: vec![TreeNode {
            mv: Some(RawMove {
                from: Square::E2,
                to: Square::E4,
                promotion: None,
            }),
            total,
            freq: [total, 0, 0, 0],
            elo_sum: 0,
            elo_count: 0,
            perf_sum: 0,
            perf_count: 0,
            year_sum: 0,
            year_count: 0,
        }],
        filter,
    }
}

fn fen_of(squares: &[(usize, char)]) -> String {
    let mut board = ['.'; 64];
    for (sq, c) in squares {
        board[*sq] = *c;
    }
    let mut fen = String::new();
    for rank in (0..8).rev() {
        let mut empty = 0;
        for file in 0..8 {
            let c = board[rank * 8 + file];
            if c == '.' {
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
        if rank > 0 {
            fen.push('/');
        }
    }
    fen.push_str(" w - - 0 1");
    fen
}

/// `n` positions with pairwise different boards (white to move): kings on
/// a1 and h8 and two white knights on the other squares.
pub fn positions(n: usize) -> Vec<Position> {
    let mut out = Vec::new();
    'outer: for i in 1..63 {
        for j in (i + 1)..63 {
            let fen = fen_of(&[(0, 'K'), (63, 'k'), (i, 'N'), (j, 'N')]);
            if let Ok(pos) = Position::from_fen(&fen) {
                out.push(pos);
                if out.len() == n {
                    break 'outer;
                }
            }
        }
    }
    assert_eq!(out.len(), n, "not enough distinct positions");
    out
}

/// Which of `probes` are served by the MAIN layer of `cache`: the cache is
/// written to a scratch file and read into a fresh one (that has an empty
/// backup layer). `len` is the number of games of the trees.
pub fn main_has(cache: &StcCache, probes: &[Position], len: u32) -> Vec<bool> {
    let (dir, base) = tmp_base();
    cache.write_to(&base, None).unwrap();
    let mut fresh = StcCache::new(Some(&base));
    let hits = probes
        .iter()
        .map(|p| fresh.lookup(p, len).is_some())
        .collect();
    drop(dir);
    hits
}

/// A `TreeCache` that records the lookups (board and side, as the first two
/// FEN fields) and counts the stores, delegating to an inner cache.
pub struct Rec<'a> {
    pub inner: &'a mut dyn TreeCache,
    pub looked: Vec<String>,
    pub stored: Vec<String>,
    /// Cancel this token after the n-th store.
    pub cancel_after: Option<(usize, mychess_db::CancelToken)>,
}

pub fn key(pos: &Position) -> String {
    pos.fen().split(' ').take(2).collect::<Vec<_>>().join(" ")
}

impl<'a> Rec<'a> {
    pub fn new(inner: &'a mut dyn TreeCache) -> Rec<'a> {
        Rec {
            inner,
            looked: Vec::new(),
            stored: Vec::new(),
            cancel_after: None,
        }
    }
}

impl TreeCache for Rec<'_> {
    fn lookup(&mut self, pos: &Position, len: u32) -> Option<CachedTree> {
        self.looked.push(key(pos));
        self.inner.lookup(pos, len)
    }

    fn store(&mut self, pos: &Position, tree: CachedTree) {
        self.stored.push(key(pos));
        self.inner.store(pos, tree);
        if let Some((n, token)) = &self.cancel_after
            && self.stored.len() >= *n
        {
            token.cancel();
        }
    }
}
