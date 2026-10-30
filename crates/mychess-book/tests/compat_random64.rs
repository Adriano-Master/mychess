//! The `Random64` table of the Scid source against the keys of the crate.
//!
//! The table is private to the crate, so this test re-implements the Polyglot
//! hash in the test (from the table parsed out of
//! `reference/scid_vs_pc-4.27/src/polyglot/random.cpp`) and compares it with
//! `polyglot_key` over many positions. It touches every table index that the
//! rules can reach. Run with `cargo test -p mychess-book -- --ignored`.

#![allow(clippy::cast_possible_truncation)]

mod common;

use std::path::PathBuf;

use mychess_book::polyglot_key;
use mychess_core::{CastlingSide, Color, Position, Role};

fn load_table() -> Option<Vec<u64>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reference/scid_vs_pc-4.27/src/polyglot/random.cpp");
    let text = std::fs::read_to_string(path).ok()?;
    let mut values = Vec::new();
    for part in text.split("U64(0x").skip(1) {
        let hex: String = part.chars().take_while(char::is_ascii_hexdigit).collect();
        values.push(u64::from_str_radix(&hex, 16).ok()?);
    }
    Some(values)
}

fn oracle(table: &[u64], pos: &Position) -> u64 {
    let mut key = 0;
    for (sq, piece) in pos.pieces() {
        let role = match piece.role {
            Role::Pawn => 0,
            Role::Knight => 1,
            Role::Bishop => 2,
            Role::Rook => 3,
            Role::Queen => 4,
            Role::King => 5,
        };
        let kind = 2 * role + usize::from(piece.color == Color::White);
        key ^= table[64 * kind + 8 * usize::from(sq.rank()) + usize::from(sq.file())];
    }
    let rights = [
        (Color::White, CastlingSide::KingSide),
        (Color::White, CastlingSide::QueenSide),
        (Color::Black, CastlingSide::KingSide),
        (Color::Black, CastlingSide::QueenSide),
    ];
    for (i, (c, s)) in rights.into_iter().enumerate() {
        if pos.has_castling_right(c, s) {
            key ^= table[768 + i];
        }
    }
    if let Some(ep) = pos.ep_square_pseudo_legal() {
        key ^= table[772 + usize::from(ep.file())];
    }
    if pos.turn() == Color::White {
        key ^= table[780];
    }
    key
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/src/polyglot/random.cpp"]
fn compat_random64_table_matches_scid_for_many_positions() {
    let Some(table) = load_table() else { return };
    assert_eq!(table.len(), 781);
    assert_eq!(table[0], 0x9D39_247E_3377_6D41);
    assert_eq!(table[780], 0xF8D6_26AA_AF27_8509);
    let mut checked = 0;
    for seed in 0..60u16 {
        let seeds: Vec<u16> = (0..120u32)
            .map(|i| (u32::from(seed) * 7919 + i * 104_729) as u16)
            .collect();
        for pos in common::random_game(&seeds) {
            assert_eq!(polyglot_key(&pos), oracle(&table, &pos), "{}", pos.fen());
            checked += 1;
        }
    }
    assert!(checked > 1000);
}
