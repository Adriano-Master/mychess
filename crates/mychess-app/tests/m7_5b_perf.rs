//! Performance of opening a large EPD file (spec `m7-5-janela-epd.md`,
//! criterion 17). Ignored: run with
//! `cargo test -p mychess-app --release --test m7_5b_perf -- --ignored`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod m7_5b_common;

use std::fmt::Write as _;
use std::time::Instant;

use m7_5b_common::*;
use mychess_core::Position;

/// `count` distinct legal positions: a black king, a white king and a white
/// rook on three squares.
fn big_epd(count: usize) -> String {
    let mut out = String::with_capacity(count * 40);
    let mut made = 0;
    'outer: for bk in 0..64_usize {
        for wk in 0..64_usize {
            for wr in 0..64_usize {
                if bk == wk || bk == wr || wk == wr {
                    continue;
                }
                let mut board = [b'1'; 64];
                board[bk] = b'k';
                board[wk] = b'K';
                board[wr] = b'R';
                let mut fen = String::new();
                for rank in 0..8 {
                    let mut empty = 0;
                    for file in 0..8 {
                        let c = board[rank * 8 + file];
                        if c == b'1' {
                            empty += 1;
                        } else {
                            if empty > 0 {
                                write!(fen, "{empty}").unwrap();
                                empty = 0;
                            }
                            fen.push(char::from(c));
                        }
                    }
                    if empty > 0 {
                        write!(fen, "{empty}").unwrap();
                    }
                    if rank < 7 {
                        fen.push('/');
                    }
                }
                for turn in [" w - -", " b - -"] {
                    let text = format!("{fen}{turn}");
                    if Position::from_fen(&format!("{text} 0 1")).is_err() {
                        continue;
                    }
                    out.push_str(&text);
                    out.push_str(" id \"x\";\n");
                    made += 1;
                    if made == count {
                        break 'outer;
                    }
                }
            }
        }
    }
    assert_eq!(made, count, "enough positions");
    out
}

#[test]
#[ignore = "performance: run in release with --ignored"]
fn a_file_of_200000_positions_opens_in_two_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.epd");
    std::fs::write(&path, big_epd(200_000)).unwrap();
    let mut rig = Rig::new();
    let started = Instant::now();
    rig.open(&path);
    let elapsed = started.elapsed();
    assert_eq!(rig.tab().records, 200_000);
    assert_eq!(rig.state.epd_rows(rig.tab().id, 199_990..200_010).len(), 10);
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}
