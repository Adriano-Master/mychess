//! Hostile-input checks of the PGN reader that complement `pgn_read_limits.rs` and
//! `pgn_read_props.rs` (which already cover random bytes, truncation of a small file at
//! every byte, 1 000 000 `(` and a 40 MiB comment): here the spec sizes (100 MB comment,
//! 50 MB line without spaces) with the *default* limits, and truncation of a richer file.
//!
//! Spec: `docs/specs/m1-5-chess960-fechamento.md`, `reader_hostile.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use mychess_pgn::read::{ParsedGame, PgnReader, ReadOptions};

fn read_all(bytes: &[u8], opts: ReadOptions) -> Vec<ParsedGame> {
    PgnReader::new(bytes, opts).filter_map(Result::ok).collect()
}

const RICH: &str = "[Event \"A\"]\n[Result \"1-0\"]\n\n1. e4 {[%clk 0:03:00]} e5 (1... c5 2. Nf3 (2. c3 $1) d6) 2. Nf3 $14 Nc6 ; line\n3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 {\ntwo lines} 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 1-0\n\n[Event \"B\"]\n[Variant \"Chess960\"]\n[SetUp \"1\"]\n[FEN \"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1\"]\n\n1. g3 g6 2. Nd3 Nd6 3. O-O O-O 4. Ne4 Ne5 *\n\n[Event \"C\"]\n\n1. d4 d5 2. c4 e6 3. Nc3 Nf6 4. Bg5 Be7 5. e3 O-O 6. Nf3 h6 7. Bh4 b6 8. cxd5 exd5 9. Bxf6 Bxf6 10. Nxd5 Bxb2 11. Nxb6 axb6 12. Rb1 Bf6 13. Rxb6 Nc6 0-1\n";

#[test]
fn truncating_a_rich_file_at_sampled_bytes_never_panics_and_prefix_games_are_stable() {
    let bytes = RICH.as_bytes();
    let reference = read_all(bytes, ReadOptions::default());
    assert_eq!(reference.len(), 3);
    for cut in (0..=bytes.len()).filter(|c| c % 3 == 0 || *c + 1 >= bytes.len()) {
        let games = read_all(&bytes[..cut], ReadOptions::default());
        assert!(games.len() <= reference.len(), "cut {cut}");
        // A game wholly before the cut is read exactly as in the full file.
        for (g, r) in games.iter().zip(&reference) {
            if g.byte_range.end < cut as u64 && r.byte_range == g.byte_range {
                assert!(g.game.same_content(&r.game), "cut {cut}");
            }
        }
    }
}

#[test]
fn single_byte_corruption_of_sampled_positions_never_panics() {
    let bytes = RICH.as_bytes();
    for i in (0..bytes.len()).step_by(11) {
        for b in [
            0u8, b'{', b'}', b'(', b')', b'[', b'"', b'\n', 0xFF, b'$', b';',
        ] {
            let mut v = bytes.to_vec();
            v[i] = b;
            drop(read_all(&v, ReadOptions::default()));
        }
    }
}

#[test]
fn eight_mebibytes_without_any_space_is_bounded() {
    let mut s = String::from("[Event \"x\"]\n\n");
    s.push_str(&"e4".repeat(4 << 20));
    s.push_str("\n\n[Event \"after\"]\n\n1. e4 *\n");
    let t = Instant::now();
    let games = read_all(s.as_bytes(), ReadOptions::default());
    assert!(t.elapsed() < Duration::from_secs(30), "{:?}", t.elapsed());
    assert!(!games.is_empty());
}

#[test]
#[ignore = "slow: 100 MB comment; run with --ignored"]
fn hundred_megabyte_comment_with_default_limits_is_bounded() {
    let mut s = String::with_capacity(101 << 20);
    s.push_str("[Event \"x\"]\n\n1. e4 {");
    s.push_str(&"c".repeat(100 << 20));
    s.push_str("} e5 *\n\n[Event \"next\"]\n\n1. d4 *\n");
    let t = Instant::now();
    let games = read_all(s.as_bytes(), ReadOptions::default());
    assert!(t.elapsed() < Duration::from_secs(60), "{:?}", t.elapsed());
    assert!(
        games.iter().any(|g| g.game.tags().event == "next"),
        "reader must resynchronise after the oversize game"
    );
    for g in &games {
        let used = g.game.node_count();
        assert!(used < 1000);
    }
}

#[test]
#[ignore = "slow: 50 MB line without spaces; run with --ignored"]
fn fifty_megabyte_line_without_spaces_with_default_limits_is_bounded() {
    let mut s = String::with_capacity(51 << 20);
    s.push_str("[Event \"x\"]\n\n");
    s.push_str(&"Nf3Nf6".repeat(50 << 20 >> 3));
    s.push_str("\n\n[Event \"next\"]\n\n1. d4 *\n");
    let t = Instant::now();
    let games = read_all(s.as_bytes(), ReadOptions::default());
    assert!(t.elapsed() < Duration::from_secs(60), "{:?}", t.elapsed());
    assert!(games.iter().any(|g| g.game.tags().event == "next"));
}

#[test]
#[ignore = "slow: 50 MB tag-like line; run with --ignored"]
fn fifty_megabyte_header_line_is_bounded() {
    let s = format!("[Event \"{}\"]\n\n1. e4 *\n", "z".repeat(50 << 20));
    let t = Instant::now();
    let games = read_all(s.as_bytes(), ReadOptions::default());
    assert!(t.elapsed() < Duration::from_secs(60));
    for g in &games {
        assert!(g.game.tags().event.len() <= 64 * 1024);
    }
}
