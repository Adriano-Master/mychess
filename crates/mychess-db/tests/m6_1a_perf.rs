//! Timings of the advanced searches over a synthetic database (decision
//! D3243). Not part of the normal run: use
//! `cargo test -p mychess-db --release --test m6_1a_perf -- --ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::field_reassign_with_default,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_support.rs"]
mod support;

use std::time::Instant;

use mychess_core::Game;
use mychess_db::si4::{GameFlags, PreparedGame, Si4Writer};
use mychess_db::{
    BoardMatch, BoardSearch, CheckTest, FilterOp, MaterialSearch, MovesSearch, PieceRange,
    SideFilter,
};
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;

/// 200 random games of up to 60 plies, repeated to `total` games.
fn database(total: usize) -> (tempfile::TempDir, mychess_db::si4::Si4Database) {
    let mut runner = TestRunner::deterministic();
    let games: Vec<Game> = (0..200)
        .map(|_| support::arb_game().new_tree(&mut runner).unwrap().current())
        .collect();
    let prepared: Vec<PreparedGame> = games
        .iter()
        .map(|g| PreparedGame::from_game(g, GameFlags::empty()).unwrap())
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("perf");
    let mut writer = Si4Writer::create(&path, 1).unwrap();
    let mut batch = Vec::new();
    for i in 0..total {
        batch.push(prepared[i % prepared.len()].clone());
        if batch.len() == 5000 {
            writer.add_prepared_batch(&batch).unwrap();
            batch.clear();
        }
    }
    writer.add_prepared_batch(&batch).unwrap();
    drop(writer);
    let db = mychess_db::si4::Si4Database::open(&path).unwrap();
    (dir, db)
}

fn time<T>(label: &str, threads: usize, games: u32, f: impl FnOnce() -> T + Send) -> T
where
    T: Send,
{
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    let started = Instant::now();
    let out = pool.install(f);
    let elapsed = started.elapsed();
    println!(
        "{label:<28} {threads:>2} thread(s): {:>8.1} ms  ({:>7.0} ns per game)",
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_nanos() as f64 / f64::from(games)
    );
    out
}

#[test]
#[ignore = "timing run: see the module documentation"]
fn timings_on_100k_games() {
    let (_dir, mut db) = database(100_000);
    let games = db.len();
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let exact = BoardSearch::new(support::pos_after("e2e4 e7e5 g1f3"));
    let pawns = BoardSearch {
        kind: BoardMatch::Pawns,
        ..exact.clone()
    };
    let mut slow_exact = exact.clone();
    slow_exact.shortcuts = false;
    let mut material = MaterialSearch::zero();
    material.white.rook = PieceRange::new(1, 2);
    material.black.rook = PieceRange::new(1, 2);
    material.white.pawn = PieceRange::new(0, 8);
    material.black.pawn = PieceRange::new(0, 8);
    let mut material_slow = material.clone();
    material_slow.shortcuts = false;
    let moves = MovesSearch::parse(
        "e4 e5 Nf3",
        SideFilter::Both,
        CheckTest::No,
        FilterOp::Reset,
    )
    .unwrap();
    let mut moves_slow = moves.clone();
    moves_slow.fast_path = false;
    for n in [1, threads] {
        time("exact (shortcuts)", n, games, || {
            support::run_board(&mut db, &exact)
        });
        time("exact (no shortcuts)", n, games, || {
            support::run_board(&mut db, &slow_exact)
        });
        time("pawns", n, games, || support::run_board(&mut db, &pawns));
        time("material (shortcuts)", n, games, || {
            support::run_material(&mut db, &material)
        });
        time("material (no shortcuts)", n, games, || {
            support::run_material(&mut db, &material_slow)
        });
        time("moves (pre-test)", n, games, || {
            support::run_moves(&mut db, &moves)
        });
        time("moves (all SAN)", n, games, || {
            support::run_moves(&mut db, &moves_slow)
        });
    }
}
