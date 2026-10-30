//! Compatibility of the writer with the six databases bundled with Scid vs PC
//! 4.27 (`reference/scid_vs_pc-4.27/bases`, read in place, never copied).
//! Run with `cargo test -p mychess-db --release -- --ignored compat`.
//! The tests skip with a warning when the folder is missing.
#![allow(
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/si4_write_support.rs"]
mod support;

use std::path::{Path, PathBuf};

use mychess_db::si4::derive::{decode_count, derive_mainline, encode_count};
use mychess_db::si4::{
    ENTRY_LEN, GameFlags, HEADER_LEN, ListPosition, NameBuilder, NameKind, PreparedGame,
    Si4Database, Si4Writer, encode_record,
};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{parse_sn4, tmp};

fn bases_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/bases");
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("warning: skipping, {} not found", dir.display());
        None
    }
}

const BASES: [(&str, u32); 6] = [
    ("endings", 67),
    ("matein1", 1212),
    ("matein2", 15_623),
    ("matein3", 7_862),
    ("matein4andmore", 3_417),
    ("tactics", 928),
];

#[rstest]
#[case("endings")]
#[case("matein1")]
#[case("matein2")]
#[case("matein3")]
#[case("matein4andmore")]
#[case("tactics")]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_record_and_entry_round_trip_byte_for_byte(#[case] name: &str) {
    let Some(dir) = bases_dir() else { return };
    let mut db = Si4Database::open(&dir.join(name)).unwrap();
    let index = std::fs::read(dir.join(format!("{name}.si4"))).unwrap();
    let mut stored_diff = 0u32;
    for n in 1..=db.len() {
        let record = db.record_bytes(n).unwrap().to_vec();
        let raw = db.raw_game(n).unwrap();
        let encoded = encode_record(&raw).unwrap();
        assert_eq!(encoded.bytes, record, "{name} game {n}: record bytes");

        let entry = db.entry(n).unwrap();
        let at = HEADER_LEN + ENTRY_LEN * (n as usize - 1);
        assert_eq!(
            entry.to_bytes().as_slice(),
            &index[at..at + ENTRY_LEN],
            "{name} {n}: entry"
        );

        // derived fields recomputed from the moves
        let standard = raw.start_fen.is_none();
        let start = match &raw.start_fen {
            None => ListPosition::standard(),
            Some(f) => ListPosition::from_fen(std::str::from_utf8(f).unwrap()).unwrap(),
        };
        let moves: Vec<_> = raw.mainline().iter().map(|m| m.mv).collect();
        let d = derive_mainline(&start, standard, &moves).unwrap();
        assert_eq!(entry.flags.contains(GameFlags::START), !standard);
        for (got, code) in [
            (entry.variation_count, encoded.variation_count),
            (entry.comment_count, encoded.comment_count),
            (entry.nag_count, encoded.nag_count),
        ] {
            assert_eq!(got, decode_count(encode_count(code)), "{name} {n}: counts");
        }
        if standard {
            assert_eq!(entry.final_mat_sig, d.final_mat_sig, "{name} {n}: material");
            assert_eq!(
                u32::from(entry.num_half_moves),
                d.num_half_moves % 1024,
                "{name} {n}"
            );
            assert_eq!(entry.home_pawn_data, d.home_pawn_data, "{name} {n}: pawns");
            assert_eq!(entry.flags.contains(GameFlags::PROMOTION), d.promotions);
            assert_eq!(
                entry.flags.contains(GameFlags::UNDER_PROMOTION),
                d.under_promotions
            );
            stored_diff += u32::from(entry.stored_line_code != d.stored_line_code);
        }
    }
    assert_eq!(stored_diff, 0, "{name}: stored line codes (D296/D309)");
    assert_eq!(db.len(), BASES.iter().find(|b| b.0 == name).unwrap().1);
}

#[rstest]
#[case("endings")]
#[case("matein1")]
#[case("matein2")]
#[case("matein3")]
#[case("matein4andmore")]
#[case("tactics")]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_name_file_is_rebuilt_byte_for_byte(#[case] name: &str) {
    let Some(dir) = bases_dir() else { return };
    let db = Si4Database::open(&dir.join(name)).unwrap();
    let file = std::fs::read(dir.join(format!("{name}.sn4"))).unwrap();

    // Rebuild from the decoded names (which carries the historic maxFrequency
    // of the file, D279) and recount the frequencies, in ID order.
    let mut b = NameBuilder::from_name_base(db.names());
    for kind in NameKind::ALL {
        for id in 0..db.names().num_names(kind) {
            let freq = db.names().frequency(kind, id).unwrap();
            b.inc_frequency(kind, id, i32::try_from(freq).unwrap());
        }
    }
    b.set_time_stamp(db.names().time_stamp());
    let ours = b.to_bytes();
    // The independent decoder agrees on structure before comparing bytes.
    let (a, f) = (parse_sn4(&ours), parse_sn4(&file));
    assert_eq!(a.num_names, f.num_names, "{name}");
    assert_eq!(a.max_freq, f.max_freq, "{name}: maxFrequency");
    assert!(ours == file, "{name}: .sn4 differs from Scid's");
}

#[rstest]
#[case("endings")]
#[case("matein1")]
#[case("matein2")]
#[case("matein3")]
#[case("matein4andmore")]
#[case("tactics")]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_every_game_survives_decode_then_write_then_decode(#[case] name: &str) {
    let Some(dir) = bases_dir() else { return };
    let mut src = Si4Database::open(&dir.join(name)).unwrap();
    let mut prepared = Vec::new();
    let mut games = Vec::new();
    for n in 1..=src.len() {
        let game = src.game(n).unwrap();
        let flags = src.entry(n).unwrap().flags & GameFlags::EDITABLE;
        prepared.push(
            PreparedGame::from_game(&game, flags).unwrap_or_else(|e| panic!("{name} {n}: {e:?}")),
        );
        games.push(game);
    }

    let (_tmp, base) = tmp();
    let mut w = Si4Writer::create(&base, 4).unwrap();
    w.add_prepared_batch(&prepared).unwrap();
    drop(w);

    let mut out = Si4Database::open(&base).unwrap();
    assert_eq!(out.len(), src.len());
    for n in 1..=out.len() {
        let back = out.game(n).unwrap();
        assert!(games[n as usize - 1].same_content(&back), "{name} game {n}");
        let (a, b) = (src.entry(n).unwrap(), out.entry(n).unwrap());
        if a.flags.contains(GameFlags::START) {
            // Scid leaves stale PROMO/UPROMO bits in games with their own start
            // position (e.g. endings game 12 has both without any promotion);
            // the writer computes them from the main line (D301).
            let keep = GameFlags::EDITABLE | GameFlags::START;
            assert_eq!(a.flags & keep, b.flags & keep, "{name} {n}: flags");
        } else {
            assert_eq!(a.flags, b.flags, "{name} {n}: flags");
        }
        assert_eq!(
            (a.result, a.date, a.eco_code),
            (b.result, b.date, b.eco_code)
        );
        assert_eq!((a.white_elo, a.black_elo), (b.white_elo, b.black_elo));
        for kind_ids in [
            (NameKind::Player, a.white_id, b.white_id),
            (NameKind::Player, a.black_id, b.black_id),
            (NameKind::Event, a.event_id, b.event_id),
            (NameKind::Site, a.site_id, b.site_id),
            (NameKind::Round, a.round_id, b.round_id),
        ] {
            assert_eq!(
                src.names().get(kind_ids.0, kind_ids.1),
                out.names().get(kind_ids.0, kind_ids.2),
                "{name} {n}: names"
            );
        }
        if !a.flags.contains(GameFlags::START) {
            assert_eq!(a.final_mat_sig, b.final_mat_sig, "{name} {n}");
            assert_eq!(a.home_pawn_data, b.home_pawn_data, "{name} {n}");
            assert_eq!(a.num_half_moves, b.num_half_moves, "{name} {n}");
            assert_eq!(a.stored_line_code, b.stored_line_code, "{name} {n}");
        }
    }
    // The rewritten name file is well formed for the independent decoder.
    parse_sn4(&support::read(&base, "sn4"));
}

#[rstest]
#[case("endings")]
#[case("matein1")]
#[case("matein2")]
#[case("matein3")]
#[case("matein4andmore")]
#[case("tactics")]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_compacting_a_copy_keeps_the_games_that_were_not_deleted(#[case] name: &str) {
    use mychess_db::si4::{FlagOp, compact_games, compact_names};
    let Some(dir) = bases_dir() else { return };
    // The copy is a temporary working file; the reference is only read.
    let (_tmp, base) = tmp();
    for e in ["si4", "sn4", "sg4"] {
        std::fs::copy(dir.join(format!("{name}.{e}")), support::ext(&base, e)).unwrap();
    }
    let mut src = Si4Database::open(&base).unwrap();
    let total = src.len();
    let all: Vec<_> = (1..=total).map(|n| src.game(n).unwrap()).collect();
    let flags: Vec<_> = (1..=total).map(|n| src.entry(n).unwrap().flags).collect();
    let header = src.header().clone();
    drop(src);

    let mut w = Si4Writer::open(&base).unwrap();
    let mut deleted = 0u32;
    for n in (1..=total).filter(|n| n % 7 == 3) {
        if w.set_flag(n, GameFlags::DELETE, FlagOp::Set).unwrap() {
            deleted += 1;
        }
    }
    let (w, report) = compact_games(w).unwrap();
    assert_eq!(report.deleted, deleted);
    assert_eq!(report.corrupt, 0);
    assert_eq!(report.kept, total - deleted);
    let (w, _) = compact_names(w).unwrap();
    drop(w);

    let mut out = Si4Database::open(&base).unwrap();
    assert_eq!(out.len(), total - deleted);
    assert_eq!(out.header().base_type(), header.base_type());
    assert_eq!(out.header().description(), header.description());
    let mut k = 0usize;
    for n in 1..=total {
        if n % 7 == 3 && !flags[n as usize - 1].contains(GameFlags::DELETE) {
            continue;
        }
        if flags[n as usize - 1].contains(GameFlags::DELETE) {
            continue;
        }
        k += 1;
        let back = out.game(u32::try_from(k).unwrap()).unwrap();
        assert!(
            all[n as usize - 1].same_content(&back),
            "{name}: game {n} as {k}"
        );
        assert_eq!(
            out.entry(u32::try_from(k).unwrap()).unwrap().flags,
            flags[n as usize - 1]
        );
    }
    assert_eq!(k as u32, out.len());
    // every name that remains is used
    let sn4 = parse_sn4(&support::read(&base, "sn4"));
    assert!(sn4.sections.iter().flatten().all(|r| r.freq > 0));
}
