//! Smoke test over the databases bundled with Scid vs PC 4.27: every game of
//! the six databases must open, convert to a `Game` and decode as a main line.
//!
//! The databases are read straight from `reference/` (outside git) and never
//! copied. Run with `cargo test -p mychess-db --test compat_si4_smoke --
//! --ignored --nocapture`. The full contract suite is the tester's.
#![allow(clippy::print_stderr, clippy::unwrap_used, clippy::too_many_lines)]

use std::path::{Path, PathBuf};

use mychess_core::{Color, Role};
use mychess_db::si4::{ConversionReport, GameFlags, MaterialSignature, Si4Database};

const BASES: [&str; 6] = [
    "endings",
    "matein1",
    "matein2",
    "matein3",
    "matein4andmore",
    "tactics",
];

/// Scid's encoding of a count into 4 bits, back to the approximate value.
fn scale(count: usize) -> u8 {
    match count {
        0..=10 => u8::try_from(count).unwrap(),
        11..=12 => 10,
        13..=17 => 15,
        18..=24 => 20,
        25..=34 => 30,
        35..=44 => 40,
        _ => 50,
    }
}

fn bases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/bases")
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_smoke() {
    let dir = bases_dir();
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    let mut failures = Vec::new();
    for name in BASES {
        let mut db = Si4Database::open(&dir.join(name)).unwrap();
        let mut opened = 0u32;
        for n in 1..=db.len() {
            let entry = db.entry(n).unwrap();
            let game = match db.game_with_report(n) {
                Ok((game, report)) => {
                    if report != ConversionReport::default() {
                        eprintln!("{name} #{n}: {report:?}");
                    }
                    game
                }
                Err(e) => {
                    failures.push(format!("{name} #{n}: game: {e}"));
                    continue;
                }
            };
            opened += 1;
            let moves = match db.decode_mainline(n) {
                Ok(moves) => moves,
                Err(e) => {
                    failures.push(format!("{name} #{n}: mainline: {e}"));
                    continue;
                }
            };
            let raw = db.raw_game(n).unwrap();
            if moves.len() != raw.mainline().len() {
                failures.push(format!("{name} #{n}: decode_mainline vs raw_game"));
            }
            // Scid does not refresh NumHalfMoves of games with their own start
            // position (MakeHomePawnList returns early), so only check the rest.
            if !entry.flags.contains(GameFlags::START)
                && usize::from(entry.num_half_moves) != moves.len()
            {
                failures.push(format!(
                    "{name} #{n}: index half moves {} vs {}",
                    entry.num_half_moves,
                    moves.len()
                ));
            }
            if game.tags().result != entry.result {
                failures.push(format!("{name} #{n}: result"));
            }
            // Counts use Scid's approximate scale.
            let variations = raw.lines.len() - 1;
            let comments = raw
                .lines
                .iter()
                .map(|l| {
                    usize::from(l.start_comment.is_some())
                        + l.moves.iter().filter(|m| m.comment.is_some()).count()
                })
                .sum::<usize>();
            let nags: usize = raw
                .lines
                .iter()
                .flat_map(|l| &l.moves)
                .map(|m| m.nags.len())
                .sum();
            for (what, real, coded) in [
                ("variations", variations, entry.variation_count),
                ("comments", comments, entry.comment_count),
                ("nags", nags, entry.nag_count),
            ] {
                if scale(real) != coded {
                    failures.push(format!("{name} #{n}: {what} {real} vs coded {coded}"));
                }
            }
            // Final material against the signature of the index.
            let last = game.mainline().last();
            let position = match last {
                Some(id) => game.position_at(id).unwrap(),
                None => game.start_position(),
            };
            let count = |color, role, cap: u8| {
                let n = position
                    .pieces()
                    .filter(|(_, p)| p.color == color && p.role == role)
                    .count();
                u8::try_from(n).unwrap().min(cap)
            };
            let expected = MaterialSignature {
                white_pawns: count(Color::White, Role::Pawn, 15),
                white_knights: count(Color::White, Role::Knight, 3),
                white_bishops: count(Color::White, Role::Bishop, 3),
                white_rooks: count(Color::White, Role::Rook, 3),
                white_queens: count(Color::White, Role::Queen, 3),
                black_pawns: count(Color::Black, Role::Pawn, 15),
                black_knights: count(Color::Black, Role::Knight, 3),
                black_bishops: count(Color::Black, Role::Bishop, 3),
                black_rooks: count(Color::Black, Role::Rook, 3),
                black_queens: count(Color::Black, Role::Queen, 3),
            };
            // Same for FinalMatSig: only reliable for games from the standard start.
            if !entry.flags.contains(GameFlags::START) && entry.final_material() != expected {
                failures.push(format!(
                    "{name} #{n}: final material {:?} vs {expected:?}",
                    entry.final_material()
                ));
            }
        }
        eprintln!("{name}: {opened}/{} games opened", db.len());
    }
    let mut kinds = std::collections::BTreeMap::new();
    for f in &failures {
        let kind = f
            .split(": ")
            .nth(1)
            .unwrap_or("?")
            .split(' ')
            .next()
            .unwrap_or("?");
        *kinds.entry(kind.to_owned()).or_insert(0u32) += 1;
    }
    eprintln!("failure kinds: {kinds:?}");
    for f in failures.iter().take(15) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} failures", failures.len());
}
