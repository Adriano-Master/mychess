//! Compatibility tests over the six databases bundled with Scid vs PC 4.27
//! (`reference/scid_vs_pc-4.27/bases`, read in place, never copied). Run with
//! `cargo test -p mychess-db --release -- --ignored compat_si4`.
#![allow(clippy::unwrap_used, clippy::print_stderr, clippy::too_many_lines)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use mychess_core::{Color, Game, Role};
use mychess_db::si4::{GameFlags, MaterialSignature, NameKind, Si4Database};
use mychess_pgn::write::{WriteOptions, game_to_string};
use pretty_assertions::assert_eq;
use rstest::rstest;

fn bases_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/bases");
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("skipping: {} not found", dir.display());
        None
    }
}

fn open(name: &str) -> Option<Si4Database> {
    Some(Si4Database::open(&bases_dir()?.join(name)).unwrap())
}

/// Scid's 4-bit count scale back to its representative value.
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

fn movetext(game: &Game) -> String {
    let text = game_to_string(game, &WriteOptions::default()).unwrap();
    let body = text.rsplit("\n\n").next().unwrap();
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[rstest]
#[case("endings", 67)]
#[case("matein1", 1212)]
#[case("matein2", 15_623)]
#[case("matein3", 7_862)]
#[case("matein4andmore", 3_417)]
#[case("tactics", 928)]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_every_game_opens_and_agrees_with_its_index_entry(
    #[case] name: &str,
    #[case] games: u32,
) {
    let Some(mut db) = open(name) else { return };
    assert_eq!(db.len(), games);
    assert_eq!(db.header().num_games(), games);
    assert_eq!(db.entries().count(), games as usize);
    let mut failures = Vec::new();
    let mut discarded = 0u32;
    for n in 1..=db.len() {
        let entry = db.entry(n).unwrap();
        let (game, report) = match db.game_with_report(n) {
            Ok(x) => x,
            Err(e) => {
                failures.push(format!("#{n}: game: {e}"));
                continue;
            }
        };
        discarded += report.discarded_variations;
        let moves = match db.decode_mainline(n) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("#{n}: mainline: {e}"));
                continue;
            }
        };
        let raw = db.raw_game(n).unwrap();
        let from_raw: Vec<_> = raw.mainline().iter().map(|x| x.mv).collect();
        if moves != from_raw {
            failures.push(format!("#{n}: decode_mainline differs from raw_game"));
        }
        // D268: games with their own start position keep stale half-move counts.
        let own_start = entry.flags.contains(GameFlags::START);
        if !own_start && usize::from(entry.num_half_moves) != moves.len() {
            failures.push(format!(
                "#{n}: half moves {} vs {}",
                entry.num_half_moves,
                moves.len()
            ));
        }
        if game.tags().result != entry.result {
            failures.push(format!("#{n}: result"));
        }
        let names = db.names();
        let white = String::from_utf8_lossy(names.get(NameKind::Player, entry.white_id).unwrap());
        if report.discarded_variations == 0
            && game.mainline().count() != moves.len()
            && !moves.iter().any(|m| m.is_null())
        {
            failures.push(format!("#{n}: Game main line length"));
        }
        if game.tags().white != white {
            failures.push(format!("#{n}: white name"));
        }
        let variations = raw.lines.len() - 1;
        let comments: usize = raw
            .lines
            .iter()
            .map(|l| {
                usize::from(l.start_comment.is_some())
                    + l.moves.iter().filter(|m| m.comment.is_some()).count()
            })
            .sum();
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
                failures.push(format!("#{n}: {what} {real} vs coded {coded}"));
            }
        }
        if !own_start {
            let position = match game.mainline().last() {
                Some(id) => game.position_at(id).unwrap(),
                None => game.start_position(),
            };
            let count = |color, role, cap: u8| {
                let c = position
                    .pieces()
                    .filter(|(_, p)| p.color == color && p.role == role)
                    .count();
                u8::try_from(c).unwrap().min(cap)
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
            if entry.final_material() != expected {
                failures.push(format!("#{n}: final material"));
            }
        }
    }
    eprintln!(
        "{name}: {} failures, {discarded} discarded variations",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} failures, first: {:?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
    assert_eq!(
        discarded,
        u32::from(name == "tactics"),
        "D275 residual case"
    );
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_total_number_of_games_is_29109() {
    let Some(dir) = bases_dir() else { return };
    let total: u32 = [
        "endings",
        "matein1",
        "matein2",
        "matein3",
        "matein4andmore",
        "tactics",
    ]
    .iter()
    .map(|b| Si4Database::open(&dir.join(b)).unwrap().len())
    .sum();
    assert_eq!(total, 29_109);
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_headers_and_names_of_the_bundled_bases() {
    for (name, kind, desc) in [
        ("endings", 16, "Endings"),
        ("matein1", 15, "Mate in 1"),
        ("matein2", 15, "Mate in 2"),
        ("matein3", 15, "Mate in 3"),
        ("matein4andmore", 15, "Mate in 4+"),
        ("tactics", 0, "Tactics"),
    ] {
        let Some(db) = open(name) else { return };
        assert_eq!(db.header().base_type(), kind, "{name}");
        assert_eq!(db.header().description(), desc.as_bytes(), "{name}");
        assert_eq!(db.header().auto_load(), Some(1), "{name}");
        assert!(db.names().num_names(NameKind::Player) >= 1, "{name}");
    }
}

/// `1.Qg6#` hand-decoded: record `FA 01 31 | F4 01 30 | 00 | 01 | FEN 00 | 24 6E 0F`.
/// Tags: SetUp=1, PlyCount=0, flags 1 (own start), FEN; white list [K d4, R h7,
/// Q e4]; `24` = piece 2 (queen), v = 4 = its column, so the diagonal form follows;
/// `6E` = 64 + 46 (g6); `0F` ends the game.
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_matein1_first_game_matches_the_hand_decoded_record() {
    let Some(mut db) = open("matein1") else {
        return;
    };
    let record = db.record_bytes(1).unwrap().to_vec();
    assert_eq!(&record[..3], &[0xFA, 0x01, 0x31]);
    assert_eq!(&record[record.len() - 3..], &[0x24, 0x6E, 0x0F]);
    let game = db.game(1).unwrap();
    assert_eq!(
        game.start_position().fen(),
        "8/7R/3k4/8/3KQ3/8/8/7q w - - 0 1"
    );
    assert_eq!(movetext(&game), "1. Qg6# *");
    assert_eq!(game.tags().event, "matein1");
    assert_eq!(game.tags().site, "problem solved");
    assert_eq!(game.tags().get_extra("PlyCount"), Some("0"));
}

/// Last game of matein1: `... 00 25 0F`; white list [K g2, R g5, B b4, ...];
/// `25` = piece 2 (Bb4), v = 5: column f, `to = from + 9 * (5 - 1) = 61` (f8).
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_matein1_last_game_matches_the_hand_decoded_record() {
    let Some(mut db) = open("matein1") else {
        return;
    };
    let n = db.len();
    assert_eq!(n, 1212);
    let record = db.record_bytes(n).unwrap().to_vec();
    assert_eq!(&record[record.len() - 2..], &[0x25, 0x0F]);
    let game = db.game(n).unwrap();
    assert_eq!(
        game.start_position().fen(),
        "7R/1p5r/p1p2n1k/3p1pR1/1B1r1Pp1/P5P1/1P3PK1/8 w - - 0 1"
    );
    assert_eq!(movetext(&game), "1. Bf8# *");
}

/// D268: game 2 of matein1 has its own start position, the index says 3 half
/// moves and the line has 1 (Scid's `MakeHomePawnList` returns early for a
/// non-standard start, `game.cpp:1387-1389`).
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_start_position_games_keep_a_stale_half_move_count() {
    let Some(mut db) = open("matein1") else {
        return;
    };
    let e = db.entry(2).unwrap();
    assert!(e.flags.contains(GameFlags::START));
    assert_eq!(e.num_half_moves, 3);
    assert_eq!(db.decode_mainline(2).unwrap().len(), 1);
    assert_eq!(movetext(&db.game(2).unwrap()), "1. Qh8 *");
}

/// `1.Nd4+ Kb7`: record `... 00 25 06 0F`; white list [K e2, B g3, N f3];
/// `25` = knight, v = 5 (+6: f3 to d4); `06` = black king v = 6 (+7: c6 to b7).
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_endings_first_and_last_games() {
    let Some(mut db) = open("endings") else {
        return;
    };
    let record = db.record_bytes(1).unwrap().to_vec();
    assert_eq!(&record[record.len() - 3..], &[0x25, 0x06, 0x0F]);
    let first = db.game(1).unwrap();
    assert_eq!(
        first.start_position().fen(),
        "8/8/2k5/8/8/5NB1/4K3/8 w - - 0 1"
    );
    assert_eq!(movetext(&first), "1. Nd4+ Kb7 *");
    let last = db.game(67).unwrap();
    assert_eq!(
        last.start_position().fen(),
        "8/8/8/1k1K4/8/2R5/3n4/8 b - - 0 1"
    );
    assert_eq!(
        movetext(&last),
        "*",
        "a record with no moves: only 0F after the FEN"
    );
    assert_eq!(db.decode_mainline(67).unwrap().len(), 0);
}

/// Record `F3 04 'Tv2R' | FA 01 '1' | F4 01 '3' | 00 | 01 | FEN 00 | 31 02 46 0D 4B 0B 12 0C 0E 0F | "Alternative" 00`.
/// White list [K h3, R b5, B d5, P g4, R f3, P c6]; black [k h6, p g6, b d6, p h4, r c2].
/// `31` = pawn g4 push (g5+); `02` = Kh5; `46` = Rf3-g3; `0D` opens a variation;
/// `4B` = Rf3-f4; `0B 12` = NAG 18; `0C` = comment mark; `0E` closes; `0F` ends.
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_tactics_game_with_variation_nag_and_comment_matches_the_hand_decoded_record() {
    let Some(mut db) = open("tactics") else {
        return;
    };
    let record = db.record_bytes(206).unwrap().to_vec();
    let mut tail = vec![0x31, 0x02, 0x46, 0x0D, 0x4B, 0x0B, 0x12, 0x0C, 0x0E, 0x0F];
    tail.extend_from_slice(b"Alternative\0");
    assert_eq!(&record[record.len() - tail.len()..], tail.as_slice());
    let game = db.game(206).unwrap();
    assert_eq!(
        game.start_position().fen(),
        "8/8/2Pb2pk/1R1B4/6Pp/5R1K/2r5/8 w - - 0 1"
    );
    assert_eq!(
        movetext(&game),
        "1. g5+ Kh5 2. Rg3 (2. Rf4 $18 {Alternative}) 1-0"
    );
    assert_eq!(game.tags().white, "Schamkowitsch");
    assert_eq!(game.tags().black, "Visier");
    assert_eq!(game.tags().date.to_string(), "1967.??.??");
    let e = db.entry(206).unwrap();
    assert_eq!((e.variation_count, e.comment_count, e.nag_count), (1, 1, 1));
}

/// Pinned samples (taken from this reader's output, checked for legality and
/// plausibility but not hand-decoded): first and last game of tactics, and the
/// D275 residual case, game 171.
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_tactics_pinned_samples_and_the_discarded_variation() {
    let Some(mut db) = open("tactics") else {
        return;
    };
    let first = db.game(1).unwrap();
    assert_eq!(first.tags().white, "Fischer");
    assert_eq!(first.tags().black, "Andersson");
    assert_eq!(
        movetext(&first),
        "1. Qxe6 Rxe6 2. f7 h6 3. Rg8+ Kh7 4. Rxf8 Kg6 1-0"
    );
    let n = db.len();
    assert_eq!(n, 928);
    let last = db.game(n).unwrap();
    assert_eq!(movetext(&last), "1... Qxf1+ 2. Kxf1 Rxf5+ 3. Qxf5 gxf5 0-1");
    let (g171, report) = db.game_with_report(171).unwrap();
    assert_eq!(
        (report.merged_variations, report.discarded_variations),
        (0, 1)
    );
    assert_eq!(g171.mainline().count(), 3);
    assert_eq!(movetext(&g171), "1. Re4 Rg8 2. Qxh7+ $1 1-0");
    assert_eq!(
        db.raw_game(171).unwrap().lines.len(),
        2,
        "raw_game keeps the variation"
    );
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_si4_matein2_opens_and_decodes_every_game_quickly_in_release() {
    let Some(dir) = bases_dir() else { return };
    let started = Instant::now();
    let mut db = Si4Database::open(&dir.join("matein2")).unwrap();
    let mut plies = 0usize;
    for n in 1..=db.len() {
        plies += db.decode_mainline(n).unwrap().len();
    }
    let elapsed = started.elapsed();
    eprintln!("matein2: {plies} plies in {elapsed:?}");
    assert!(plies > 0);
    if !cfg!(debug_assertions) {
        assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
    }
}
